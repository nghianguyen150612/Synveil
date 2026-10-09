# Synveil v0.2 installation product contract

Status: **Normative v0.2 target contract — Prompt001; implementation deferred**.
Theme: **Effortless Setup**. Baseline: released `v0.1.0`, source commit
`fa23232ff0154f627ebdd221ec5435134f177af0`, product version `0.1.0`.

This document defines the installation and first-run experience that later
v0.2 prompts must deliver. It does not describe new capabilities as shipped.
The [v0.1 package policy](../en/RELEASE_PACKAGING.md) remains the guide for
current artifacts. [ADR-049](../adr/ADR-049-v0.2-effortless-installation-architecture.md)
records the architectural decisions; the [roadmap](ROADMAP.md) assigns scope.
Released-data compatibility and accepted ownership/security ADRs remain binding.

## 1. North-star journey and release bar

A normal user on a clean supported computer must be able to follow:

```text
download Synveil → install → launch → guided setup
→ select/create the first library → begin synchronization
```

The ordinary desktop journey must require no terminal and no engineering
documentation. Approximately five minutes to usable Synveil is a usability
target when the required server, credentials, storage and connectivity are
available; it is not yet a measured benchmark or a guarantee about downloads,
host provisioning, remote access, or completion of an initial file transfer.

Documentation may explain advanced administration. It must not supply missing
steps in ordinary installation acceptance. Terminal-only installation cannot
be the sole desktop experience. A clean-machine result must name the exact OS,
architecture, artifact and version tested. Cross-builds, package inspection,
fixtures and offscreen launches do not establish native clean-machine success.

The normal path uses simple language, safe defaults, few decisions, native
platform conventions, progressive disclosure, reversible automation,
recoverable failure, and discoverable repair and uninstall.

## 2. Normative UX principles

| Principle | Required behavior |
| --- | --- |
| A — Sensible defaults | Choose a safe common path; reuse saved choices during repair/update. |
| B — Hide implementation details | Do not require users to understand PostgreSQL, SQLite, Qt, systemd, Task Scheduler, IPC, object storage or migration ledgers. |
| C — Progressive disclosure | Show Advanced controls only after the user deliberately opens them. |
| D — Clear primary action | Give each screen one obvious next action where practical, with clear Back/Cancel behavior. |
| E — Safe automation | Automate only operations with known ownership, consent, validation and recovery. |
| F — Recoverable failure | Explain what happened, whether action is needed, and the safe action; preserve state. |
| G — Native expectations | Respect Windows and each Linux format's install, permission and removal conventions. |

Keyboard navigation, focus order, accessible names, readable progress and
screen-reader status must work through success and recovery. English and
Vietnamese product copy must convey the same actions, limits and consequences.
Internal typed names and core state models remain unchanged; this is
presentation policy under [ADR-022](../adr/ADR-022-progressive-disclosure-complexity-boundary.md).

## 3. Authoritative distribution matrix

All rows below are **v0.2 delivery targets**, conditional on later native
acceptance. Existing v0.1 DEB/RPM and Windows ZIP artifacts do not prove the
v0.2 journeys. No new artifact is created by Prompt001.

| Platform / architecture | Primary artifact | Portable / alternate | Primary UX |
| --- | --- | --- | --- |
| Windows 11 AMD64 / x86_64 | `SynveilSetup.exe` | Versioned portable ZIP if retained; Advanced distribution | GUI installer |
| Ubuntu 24.04 x86_64 (`debian-x86_64` profile) | Versioned `.deb` | Supported Linux install script | Native graphical package installation |
| Fedora 42 x86_64 | Versioned `.rpm` | Supported Linux install script | Native graphical package installation |
| AppImage on Ubuntu 24.04 / Fedora 42 x86_64 | Versioned `.AppImage` | — | Portable GUI application |

Current Linux qualification is authoritative in
`deploy/install/linux-platforms-v1.json`: Debian is detected-not-qualified.
These exact targets remain conditional on native acceptance; P045 does not
claim they have passed. Historical family headings do not broaden qualification.

Windows ARM64, Linux ARM64/aarch64, all 32-bit targets, other CPU architectures,
macOS, iOS and Android are **outside the v0.2 Prompt001 installation matrix**.
Declarative ARM package-name mapping in existing scripts is not support proof.
RPM format support does not imply support for every RPM-based distribution.
AppImage does not imply compatibility with every Linux kernel, libc or desktop.

Distribution foundation must freeze the exact Windows versions, distro
versions, Linux runtime baseline, GUI package-handler availability and AppImage
execution requirements before advertising support. Missing native dependency
or graphical install support fails qualification for the corresponding row.
Each release download page must offer the primary artifact first, explain the
alternate's purpose, and avoid presenting ZIP as the standard Windows install.

## 4. Normal installation surfaces

### Windows

```text
Download SynveilSetup.exe → double click → Welcome
→ Terms / install options → Installing → Finish → Synveil opens
```

The standard installer installs the desktop and independent background client,
their verified runtime dependencies, and application integration. Prefer a
per-user destination outside configuration/state directories and current-user
integration. Do not request administrator elevation for that ordinary path.
Any later machine-wide option must be explicit, separately qualified, and
explain its actual need for elevation. The retained ZIP is an advanced portable
artifact with its own tested limits, not a substitute for this journey.

### Debian / Ubuntu and Fedora / RPM

```text
Download DEB or RPM → double click → native package application
→ Install → open Synveil from the application menu
```

Use the native package manager for dependency resolution, package ownership
and its visible privilege prompt. Do not impose a second custom wizard over
the package application. The application menu entry and icon are required.
Hooks retain the existing non-starting policy: no unattended user-autostart
enablement, server provisioning, database migration or credential creation.
User-level preferences are offered in first-run setup where the package UI
cannot collect them. Launch occurs as the signed-in user, never as root. Prompt019 implements the bounded login-startup choice while separately
ensuring current-session client availability; its focused result is
`APPLICATION_LAUNCHED`, not completion of connection, authentication, library,
or synchronization setup.

### Generic Linux AppImage

```text
Download AppImage → open → Synveil opens
```

The qualified environment must permit a graphical Open path, including any
necessary native permission/Trust-and-Run action with clear guidance. A terminal
`chmod` instruction cannot be required for acceptance. Runtime closure must be
tested without Qt developer packages. If the baseline requires a missing FUSE
or other component, provide a qualified GUI-compatible solution or mark that
environment unsupported. Running is user-level; system installation is not a
prerequisite. Optional application integration must be explicit and removable.
Moving the portable file must preserve state and detect stale integration.

### Supported Linux terminal installer

One documented command may download and invoke a verified installer; this is
an alternate terminal path, not a dependency of the GUI journeys. Prefer the
safer equivalent of `curl ... | sh`: obtain the script, authenticate it using
an independently trusted release identity, then execute the verified file.
No download URL or executable command is invented in this blueprint.

The later script must:

1. Detect OS/distribution/version and architecture without guessing from a
   package filename; reject unsupported combinations before mutation.
2. Select only a published compatible package from authenticated release
   metadata; use DEB/RPM through the native manager where qualified.
3. Download to a private staging location with bounded retry/timeout behavior.
   Verify version, architecture, manifest/signature and artifact digest before
   execution or installation. A digest fetched from the same unauthenticated
   source is insufficient trust; verification failure must fail closed.
4. Present the install plan, data-preservation policy and any privileged action.
   Use a visible package-manager privilege request; never hide `sudo`, embed a
   password, or silently switch to another install mode.
5. Install safely, verify the resulting payload/integration, and report a typed
   failure with an actionable explanation and meaningful exit status.
6. On rerun, inspect interrupted state and resume a verified operation or offer
   repair. Do not overwrite unknown state or continue after failed verification.

Bootstrap-script trust, release signing, package publication and key rotation
are distribution-foundation decisions before implementation. The existing
package-neutral [install layer](../../deploy/install/README.md) remains a
separate mechanism; it is not already this public quick-install script.

## 5. Minimal installer and ordinary options

Use at most four ordinary Windows installer screens unless a concrete
platform requirement justifies another:

| Screen | Content and primary action |
| --- | --- |
| Welcome | Explain that Synveil will be installed; **Continue**. |
| Terms / install options | Link the current [MIT license](../../LICENSE) and [third-party notices](../../deploy/NOTICE), show the few justified choices and location summary; **Install**. Do not invent new legal terms. |
| Installing | Show the current human-readable stage and bounded progress; permit safe cancellation at defined boundaries. |
| Finish | Display success only after verification; **Finish**, honoring the launch choice. |

Linux native packages use their native screens; the same product choices move
to first-run when needed. AppImage has no obligatory installation wizard.

| Ordinary choice | Fresh-install default | Why the user decides |
| --- | --- | --- |
| **Start Synveil when I sign in** | Selected, visibly presented; applying it requires the user's Install/Continue action | Controls background synchronization after later sign-ins. |
| **Create a desktop shortcut** | Selected where that is a native convention | Controls an extra desktop shortcut; the standard application-menu entry is required integration. |
| **Open Synveil after installation** | Selected where the installer can launch as the user | Controls the immediate post-install launch. |

A format that cannot show these choices must not silently apply a default
autostart choice: first-run obtains the explicit action before enabling it.
Repair/update preserves an existing explicit disable and other saved choices.
Avoid extra pages for components, ports or service mechanisms. Every additional
ordinary option needs a documented user decision and safe default.

The ordinary installer must not ask for a systemd unit, Task Scheduler XML,
IPC directory, SQLite location, PostgreSQL URL, Qt plugin path or server bind
address. Installation installs Synveil; it does not ask the user to design a
deployment or provision a server.

## 6. Install once; choose Host or Connect in first-run

The installer installs the desktop/client product. The first-run wizard then
offers the product-level distinction:

```text
Welcome to Synveil
How would you like to use Synveil?

[ Host Synveil on this device ]
  Store and manage your Synveil files here, for your other devices to use.

[ Connect to an existing Synveil server ]
  Use a Synveil that is already set up.
```

Do not mention PostgreSQL on this screen. Host setup may acquire additional
verified components after the user chooses Host, through a separate bounded
setup coordinator. Exact component distribution is deferred. Connect users
must not download/provision a database or server merely to install the client.

This separates native package installation from user-owned credentials and
server setup, avoids elevated first-run execution, and permits host recovery
inside the application. Canceling host setup leaves a usable installed client;
the user can resume or choose Connect. Existing profiles open their normal
state rather than being forced through a fresh setup wizard after repair/update.

### Connect

Ask for the server address using product language, validate it with the
existing origin/security policy, authenticate through the existing credential
owner, choose/create a library as authorized, choose the local folder and use
the existing safe existing-folder onboarding before beginning synchronization.
Never invent new authentication protocols or reinterpret an unavailable root
as deleted content. Server identity, credentials and local binding remain
under their existing owners.

### Guided hosting

The ordinary Host path asks:

- **Where should Synveil store your files?** Offer a safe writable location and
  a folder picker; distinguish stored server files from a synchronized folder.
- **Should Synveil start automatically?** Explain sign-in-only versus any
  separately supported always-on host behavior before asking for consent.
- **How will your other devices reach this Synveil?** Offer only verified access
  modes and explain their reach in ordinary language.

Then present a plain setup plan, provision approved host components, initialize
protected credentials and compatible database state, verify local server
readiness, guide administrator setup, and choose/create the first library.
Progress must distinguish **Installing Synveil**, **Setting up this device**
and **Ready to synchronize**; none substitutes for the others.

The target uses a managed infrastructure adapter outside the domain core,
keeping PostgreSQL as the canonical server metadata authority. It must own
private/approved database provisioning, least-privilege credentials, health,
service lifecycle, backup/restore and compatible upgrades without exposing
database administration on the ordinary path. SQLite remains client state,
not a second production server model.

The concrete database distribution must be decided by accepting or superseding
[ADR-019](../adr/ADR-019-managed-postgresql-lifecycle.md) and closing
`OD-PLAT-001` before provisioning code. Host supervision/elevation must close
the host portion of `OD-PLAT-002`. This contract does not accept a bundled
PostgreSQL implementation or install it now.

Begin setup in a trusted local/private context. Do not open public listeners,
change firewall/router rules or create an Internet relay implicitly. Qualify
LAN/direct/other access modes before offering them; show when remote access
requires further setup. Close `OD-PLAT-003` and accept/supersede
[ADR-020](../adr/ADR-020-self-hosted-first-remote-access.md) before introducing
new remote-access mechanisms. No proprietary hosted service may become a
silent mandatory dependency. Manual reverse-proxy configuration cannot be
the only ordinary hosting path for an advertised access mode.

### Advanced hosting

A deliberately opened **Advanced setup** surface may offer external PostgreSQL,
custom database URL, custom object root, custom bind address/public origin,
external TLS/reverse proxy and manual service integration. It must explain
ownership and validation before applying changes, protect/redact secrets,
and obey the same backup, migration and recovery rules. A user may return to
the normal path without losing validated choices.

Advanced mode is never the normal critical path or a workaround counted as
successful ordinary hosting. A platform with unfinished managed provisioning
cannot advertise FIRST-RUN-2 as complete.

## 7. Platform-neutral installation engineering model

This is a conceptual contract, not a new Rust type/API or custom package manager:

```text
Preflight → Plan → Acquire → Verify → Install → Integrate
→ VerifyInstallation → Complete
```

| Stage | Required evidence before advancing |
| --- | --- |
| Preflight | Supported platform/architecture, capacity, permissions, existing version/state, active processes and interrupted operation identified without mutation. |
| Plan | Versioned payload, path ownership, dependencies, user choices, privilege scope and recovery plan are explicit. |
| Acquire | Complete bounded downloads in private staging; partial files are not executable payloads. |
| Verify | Authentic release identity, digests, version/architecture and safe payload paths pass before mutation. |
| Install | Replace only validated package-owned paths through native manager/atomic file operations; preserve durable state. |
| Integrate | Apply only declared shortcuts/menu entries and explicitly chosen lifecycle integration; record completed effects. |
| VerifyInstallation | Required desktop/client binaries and runtime closure, layout, ownership/modes, menu/shortcut and chosen supervisor configuration pass validation. |
| Complete | Durable completion evidence exists; show Finish and optionally launch as the user. |

Local payload may satisfy Acquire, but verification cannot be skipped. Package
managers remain authoritative for their transactions and ownership; do not
introduce a parallel dependency solver. Install/repair orchestration must use
the existing canonical manifest/lifecycle owner and platform adapters. Host
bootstrap has a separate resumable plan and readiness result after installation.

Use a finite conceptual result set:

| Result category | Safe next action |
| --- | --- |
| `Complete` | Finish/Open; installation verification has passed. |
| `UnsupportedPlatform` | Explain the supported OS baseline; choose a supported device/artifact. |
| `UnsupportedArchitecture` | Obtain an artifact for a supported CPU; do not guess/fallback silently. |
| `InsufficientDiskSpace` | Show required capacity and ask the user to free space or change an allowed destination. |
| `PermissionRequired` | Explain the exact privileged operation and offer the native prompt or cancel. |
| `DownloadFailed` | Retry acquisition within bounds when safe; retain the last healthy installation. |
| `IntegrityVerificationFailed` | Reject the payload; obtain a newly verified official artifact. No bypass button. |
| `PackageInstallFailed` | Inspect current package state; use the manager's recovery or Repair. |
| `RuntimeDependencyFailed` | Repair the runtime closure or use the qualified native dependency path. |
| `IntegrationFailed` | Reconcile the specific owned integration, then repair it. |
| `VerificationFailed` | Offer Repair; do not show installation success. |
| `Interrupted` | Inspect durable progress, resume verified work or offer Repair. |

Each result records a stable category, stage, whether action is needed, known
completion evidence and allowed recovery. It must not collapse into an untyped
boolean or unbounded diagnostic string. Cancellation is `Interrupted` with
known cancellation context, never Complete. If a mutation's outcome is unknown,
reconcile authoritative manager/filesystem state before deciding the result;
never blindly repeat the mutation. This does not change the existing core
`OutcomeUnknown` model.

Verification permits a fresh unconfigured profile. It must not require signing
in, database provisioning, active synchronization or silently enabling login
autostart to call installation complete. If post-install launch later fails,
retain installation evidence and offer Repair with a separate launch failure;
do not claim that the application opened or that first-run completed.

## 8. Privileges, autostart and two-process lifecycle

Windows ordinary installation is per-user and least-privilege where feasible.
Linux native package changes may require elevation through the package manager;
AppImage execution and user integration normally do not. A verified installer
script explains elevation before requesting it. Privileged helpers, if later
needed for hosting, must be narrowly scoped and separately reviewed; first-run
must not run the whole desktop as administrator/root.

User copy is **Start Synveil when I sign in**. The existing desktop client
integration may use Linux `systemd --user` or current-user Windows Task
Scheduler with its existing least-privilege identity and fixed executable
resolution. Never promote that client to a root/system service or `SYSTEM` task.
Host services are a separately governed lifecycle, not that checkbox's hidden
meaning. A user-session-only host must clearly explain its sign-in dependency.

Preserve [ADR-041](../adr/ADR-041-production-desktop-launch-orchestration.md):
desktop lifecycle is not client lifecycle. Closing/Quitting the GUI does not
stop synchronization; reopening reuses the existing background client and its
writer lock. Repair/update/uninstall deliberately coordinate affected processes
through their owners, rather than killing them as a side effect of GUI close.

## 9. Ownership and lifecycle policy

Every install plan must classify paths before mutation. Never infer ownership
from proximity to a binary or from a recursive directory traversal.

| Ownership class | Examples / canonical owner | Repair / update | Ordinary uninstall |
| --- | --- | --- | --- |
| Package-owned files | Desktop/client executables, dependency closure, immutable templates, notices; Linux `PACKAGE` manifest entries | Restore/replace verified payload only | Remove known owned files/integration safely |
| Application state | Profile IDs, bindings, sync checkpoints, intents, conflicts, pause state, pending setup; existing client state owners | Preserve and validate through compatible owners | Preserve |
| User configuration | XDG configuration / `%APPDATA%\Synveil`; administrator `/etc/synveil` configuration | Preserve choices and administrator edits | Preserve |
| Credentials | OS SecretStore entries and references; administrator `/etc/synveil/credentials`; future managed host credentials | Preserve protection and identity; never export bytes into logs | Preserve by default; no implied revocation/deletion |
| Database state | Client SQLite in the user state boundary; PostgreSQL owned by the host/database operator | Compatible migration only through canonical database owner | Preserve; do not drop/reset |
| Synced library content | User-selected folders, root markers and external volumes | Preserve files and bindings; unavailable root is not deletion | Preserve; never classify as package-owned |
| Server object storage | Chosen host file storage/object roots; external pools | Preserve together with database/configuration/key identity | Preserve; never recursively remove with package files |
| Backup copies | Independent restore destinations | Preserve and verify before dangerous migration | Preserve |
| Cache / ephemeral runtime | Documented cache and per-activation IPC/runtime paths | Recreate only when proved disposable and inactive | Remove only known safe ephemeral artifacts; no recursive state cleanup |

The authoritative current mappings are the
[package manifest](../../deploy/install/MANIFEST),
[package policy](../en/RELEASE_PACKAGING.md) and
[upgrade safety](../en/UPGRADE_SAFETY.md). Future per-user install directories
must remain separate from `%APPDATA%`/`%LOCALAPPDATA%` state and XDG state.
Path containment, symlink-safe operations, restrictive credential permissions,
and preservation of shared parents apply to every adapter.

### Ordinary uninstall and explicit removal

**Uninstall Synveil** removes binaries and owned integration, coordinating
affected processes first. Preserve configuration, credentials, databases,
profiles, synchronization state, library content, server files and backups.
Explain that reinstall can reuse this state. Do not remove other users' state,
external services or shared runtimes that Synveil does not own.

**Remove Synveil data permanently** is a separate destructive operation, never
hidden behind a generic Uninstall button. It must first show the exact classes
and locations to be removed, distinguish local application state from local
library/server content, explain credential and other-device effects, and require
an explicit confirmation naming the destructive action. Any later content
deletion tool requires its own reviewed ownership/backup contract. No current
purge flag is reinterpreted as permission to erase external data.

Preserve the existing limited administrative purge boundary from
[ADR-024](../adr/ADR-024-linux-package-lifecycle-and-data-preserving-uninstall.md):
allowlisted Synveil-owned `/etc/synveil` and `/var/lib/synveil` only; never
external library/object/backup roots or PostgreSQL. Native DEB purge/RPM erase
remain data-preserving under the v0.1 policy. Later GUI deletion requires the
separate confirmation above, not an automatic extension of package removal.

### Repair

Repair must remain reachable when the installed desktop cannot start. Define
these GUI entry points during platform implementation:

| Format | Repair entry point | Upgrade / removal entry point |
| --- | --- | --- |
| Windows installer | Rerun verified `SynveilSetup.exe` → **Repair Synveil**, with the installed compatible version identified | Verified newer setup or native registered application maintenance; ordinary Windows application uninstall. |
| DEB / RPM | Native graphical package application → reinstall/repair the same verified package, with Synveil guidance naming that action | Native graphical package update/removal; use its transaction/recovery authority. |
| AppImage | Open a healthy verified Synveil repair/download surface that can replace the damaged portable payload without requiring the damaged executable | Guided user-invoked verified replacement for a supported upgrade; ordinary removal deletes the portable payload and removes only opted-in owned integration. |

The Linux phase must select and qualify the AppImage repair/upgrade surface;
manual terminal replacement or a sequence of reinstall instructions does not
satisfy these GUI lifecycle targets. User-invoked maintenance does not authorize
an unattended auto-updater. All formats revalidate the result and retain the
state outside the payload destination.

**Repair Synveil** inspects installed version/ownership, acquires the matching
or explicitly compatible verified payload, restores package-owned files and
required runtime/integration, and verifies health. Keep profiles, credentials,
databases, library bindings/content, sync state and pause/autostart preferences.
Repair is not database reset, credential rotation, library rebinding or an
unannounced upgrade. Missing/unsafe/unrecognized user state is preserved and
reported to its owner; never overwrite it to make a health probe pass.

### Upgrade and compatibility

The architecture must allow supported `v0.2.x → newer version` upgrades through
the installer/native manager without manual reinstall instructions or state
deletion. Support windows and source-version fixtures must be named before
release; there is no arbitrary downgrade/client-server compatibility promise.
Include a v0.1.0 preservation/upgrade fixture before promoting v0.2.

Preserve [ADR-021](../adr/ADR-021-safe-update-uninstall-migration.md) and the
[Prompt115 migration safeguards](../PROMPT115_UPGRADE_AUDIT.md): immutable
historical SQL, single-writer/advisory locks, complete checksum-valid ledgers,
transactional migration/version records, rejection of unknown/future schema,
and evidence preservation on failure. Verify a coordinated database/object/
configuration/secret backup before dangerous migration. Package replacement
does not itself run a migration; the canonical startup/migration owner does.

Drain/stop affected work safely, replace package files, start compatible
components and verify identity, bindings, checkpoints, pending/ambiguous work,
conflicts and pause state before resuming synchronization. An uncertain request
retains its identity and reconciles rather than being replayed as new work.
Binary rollback is permitted only for a compatible schema; otherwise use a
verified coordinated restore or compatible software. Never reset a database
to make an older binary start. Whole-package transactional rollback is not a
current v0.1 guarantee and cannot be claimed merely from atomic file replacement.

## 10. Interruption and recovery

Later coordinators must durably record a bounded operation identity, verified
payload/version, ownership plan, completed stages and verification outcome
without secrets. Consult native package state as the authority on rerun.
Progress/journal persistence must itself have a defined atomicity boundary;
a missing/corrupt record triggers inspection, not assumed completion.

| Interruption point | Rerun / recovery requirement |
| --- | --- |
| Download | Discard or safely resume private partial bytes; reverify the complete artifact. Leave installed files alone. |
| Verification | Repeat authentication/integrity checks; never install an unverified staging tree. |
| File replacement | Detect mixed/missing package files; resume manager recovery or restore the complete compatible payload with Repair. Preserve durable state. |
| Desktop integration | Reconcile known shortcuts/menu entries idempotently; remove only stale owned entries. |
| Service integration | Query the authoritative user/host supervisor; avoid duplicate tasks/services, preserve explicit disable, repair only scoped definitions. |
| Server bootstrap | Inspect component/database/bootstrap state, retain administrator/library identity and secrets, resume through canonical owners; no second database, admin account or library. |

No partial installation or interrupted host setup may silently present itself
as healthy. State the incomplete stage and offer **Resume setup**, **Repair
Synveil**, or a native manager recovery action as appropriate. Cancellation
after a mutation boundary must reconcile effects before promising cleanup.
Rollback/cleanup can touch only proven package-owned disposable resources.
Physical power-loss acceptance must later be tested separately from injected
process termination or staged-root fixtures.

## 11. Error explanations and diagnostics

Keep typed engineering categories below product copy. User-facing messages
state the condition and safe action without paths, SQL, secrets or enum names.

| Engineering condition | Ordinary message / action |
| --- | --- |
| Qt platform plugin unavailable / `RuntimeDependencyFailed` | “Synveil couldn't start correctly. Repairing the installation may fix this problem.” **Repair Synveil** |
| `DownloadFailed` | “Synveil couldn't finish downloading. Check your connection and try again.” **Try download again** |
| `IntegrityVerificationFailed` | “This download couldn't be verified. Download a fresh copy from the official source.” **Get a new download** |
| `PermissionRequired` | “Your computer needs permission to install Synveil.” Explain the scoped change; **Continue** opens the native prompt. |
| `VerificationFailed` / partial payload | “Installation is incomplete. Repair Synveil to finish setting it up.” **Repair Synveil** |
| Unknown operation outcome | “Synveil is checking whether setup finished.” Wait/reconcile; do not offer a blind repeat. |
| Unavailable local library folder | “Your local folder is unavailable. Reconnect it, then check again.” **Check again**; do not suggest deletion or reset. |

An explicit **Details** surface may show bounded, redacted diagnostic category,
stage and support reference. Define byte/entry limits and redaction before
implementation. Never show credentials, database URLs with secrets, tokens,
cookies, private keys, raw command output or unrestricted filesystem paths.
Do not add telemetry or upload diagnostics as an installation side effect.

## 12. Authoritative product terminology

| Internal concept (unchanged) | Ordinary user-facing term | Presentation rule |
| --- | --- | --- |
| client process / `synveil-client` | Background synchronization | Explain activity and availability, not process management. |
| server origin | Synveil server address | Use only where the user supplies/connects to an existing server. |
| root path / root binding | Local folder | Use a picker; distinguish server storage from synchronized local folders. |
| object root / object store | Where Synveil stores your files | Host storage choice; backend details remain Advanced. |
| `SecretStore` | Secure sign-in storage | Explain when unavailable; never reveal stored values. |
| background supervisor | Start Synveil when I sign in / Keep synchronization running | Show the user preference or recovery action, not platform machinery. |
| `OutcomeUnknown` | Checking whether the change finished | Waiting/reconciliation; never label it a safe retry. |
| `RootUnavailable` | Local folder unavailable | Ask for reconnection/Check again; retain bindings and content. |
| PostgreSQL / server database | Managed by Synveil | No database-choice question on ordinary setup; technical name allowed in Advanced. |
| SQLite / migration ledger | Saved Synveil settings and synchronization progress | Preserve; incompatible-state recovery does not mean reset. |
| IPC / Qt plugin / systemd / Task Scheduler | Connection to background synchronization / installation components | Keep raw mechanism names in bounded Advanced diagnostics only. |
| explicit purge | Remove Synveil data permanently | Separate destructive intent and precise scope; never an ordinary uninstall synonym. |

## 13. Exact acceptance journeys for later automation

For each journey use a disposable clean native machine/VM with no developer
toolchain or pre-provisioned Synveil state, unless the journey explicitly
requires installed state. Record OS/version/architecture, artifact identity,
choices, privilege prompts, stages/results and state snapshots. Use only
synthetic non-secret evidence in logs. GUI journeys permit no terminal or
engineering-guide step. Human usability review must accompany UI automation.

| ID | Preconditions and exact path | Required observable assertions |
| --- | --- | --- |
| **INSTALL-JOURNEY-1** | Qualified clean Windows x86_64 → `SynveilSetup.exe` → standard per-user install → Finish/Open | No terminal/admin for the qualified per-user path; verified desktop/client/runtime/integration; opens first-run as current user; choices honored. |
| **INSTALL-JOURNEY-2** | Qualified clean Debian/Ubuntu x86_64 desktop → DEB → graphical package app → Install → menu Open | Native privilege prompt only; declared dependencies installed without developer tooling; first-run opens; no silent user-unit enablement. Run on every advertised distro/version. |
| **INSTALL-JOURNEY-3** | Qualified clean Fedora/RPM-family x86_64 desktop → RPM → graphical package app → Install → menu Open | Native privilege prompt, dependency closure and launch pass; no silent autostart. Qualify every advertised RPM-family distro/version separately. |
| **INSTALL-JOURNEY-4** | Qualified clean generic Linux x86_64 → AppImage → graphical Open, including native execute approval if needed | No terminal/root/developer Qt requirement; first-run opens with validated runtime; no mandatory system integration. Moving/rerunning retains state and detects stale optional shortcuts. |
| **INSTALL-JOURNEY-5** | Qualified Linux terminal → one documented verified installer command → installed Synveil | Platform/architecture and trusted-download checks precede mutation; visible scoped elevation; verified package result, meaningful failure exits and rerun behavior. Tampered payload fails without installation. |
| **INSTALL-JOURNEY-6** | Installed Synveil with populated synthetic profiles/credentials/database/library state → damage a package-owned file → Repair | Restores verified payload/runtime/integration and launch health; canonical state, identity, library/server content and saved preferences preserved. Exercise each advertised format's repair entry point. |
| **INSTALL-JOURNEY-7** | Installed Synveil with the same populated fixture → ordinary native uninstall | Binaries and owned integration removed; configuration/credential/database/sync/library/server/backup state retained; reinstall reuses state. No destructive choice is implicit. |
| **INSTALL-JOURNEY-8** | Install or host setup → interrupt at each stage in section 10 → rerun | Incomplete state is detected, verified resume/repair converges, no false success/duplicate integration/admin/library, durable state retained. Test process termination and physical/VM power loss as separately labeled evidence. |

Record before/after content digests and canonical identity/value snapshots for
preservation; harmless runtime timestamps are not required to stay byte-identical.
For each mutating journey also cover cancellation, insufficient capacity,
permission denial and unsupported platform/architecture where applicable.
Package inspection alone cannot replace GUI launch acceptance.

| ID | Preconditions and exact path | Required observable assertions |
| --- | --- | --- |
| **FIRST-RUN-1** | Installed Synveil → Open → Connect to an existing Synveil server → authenticate → choose/create authorized library → select local folder → begin synchronization | Qualified reachable server and credentials available; no terminal/infrastructure questions; canonical credentials/bindings created once; safe existing-folder checks; synthetic file synchronizes and status is truthful. |
| **FIRST-RUN-2** | Installed Synveil → Open → Host Synveil on this device → guided server setup → administrator setup → local server ready → first library | Only storage/autostart/access product choices; managed prerequisites automated through approved owners; protected credentials and verified readiness; no manual PostgreSQL/env/service/reverse-proxy step; first library works and synchronization can begin. Access reach and remaining remote setup are explicit. |

Installation acceptance and first-run acceptance are separate gates. An
installation may be healthy while a server is unavailable or host setup awaits
user input. The approximate five-minute target must be measured separately
with stated download/connectivity/storage conditions, not inferred from tests.

## 14. Prompt001 completion boundary

Prompt001 creates the contract, architectural record, scope roadmap and audit
manifest plus necessary navigation only. It implements no installer, AppImage,
download script, PostgreSQL bundle, host provisioning or UI/runtime change;
changes no sync/authentication protocol, dependency, release asset or tag.
No migrations are added, edited or reordered. Product version stays `0.1.0`;
server migrations stay `36`, client migrations `7`, `LOCAL_SCHEMA_VERSION = 7`.

The [Prompt001 manifest](PROMPT001_MANIFEST.md) records the exact paths and
documentation-only checks. Future implementation requires its own bounded
prompt and evidence; accepting this target contract does not imply delivery of
any v0.2 journey. Release hardening must pass all applicable native journeys,
data-preserving repair/update/uninstall, interruption recovery and the
no-terminal release bar before v0.2 is advertised as effortless installation.
