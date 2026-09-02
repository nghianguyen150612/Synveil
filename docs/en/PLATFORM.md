# Cross-platform product and platform architecture

Status: **PLANNED normative blueprint**

This document defines the platform, distribution, onboarding, lifecycle, and
user-complexity boundaries for Synveil. It complements
[PRODUCT.md](PRODUCT.md), [ARCHITECTURE.md](ARCHITECTURE.md),
[DEPLOYMENT.md](DEPLOYMENT.md), [STORAGE.md](STORAGE.md), and
[SECURITY.md](SECURITY.md). It does not claim that installers, native clients,
managed services, or remote-access infrastructure exist.

## Product north star

Synveil is intended to become **a private cloud anyone can self-host**:

> Synveil should make owning your own cloud feel like using a normal consumer
> application, without taking ownership or advanced control away from the user.

The product serves technically comfortable users, power users, families, home
users, students, creators, developers, small teams, and non-technical users.
Non-technical users are a first-class target audience, not a later niche.

The desired default journey is:

```text
download Synveil
    ↓
install
    ↓
choose where data should live
    ↓
create an account
    ↓
connect a phone or laptop
    ↓
private cloud ready
```

The primary experience must not require an ordinary user to understand
containers, PostgreSQL, Btrfs, reverse proxies, TLS certificates, environment
variables, port forwarding, or filesystem mount options. Those mechanisms may
exist underneath the product and remain visible to advanced operators.

## Product principles

The following principles apply to product design, architecture, roadmap,
release packaging, and support documentation:

1. **Easy by default.** Safe common paths use installers, guided choices,
   automatic validation, sensible defaults, and understandable recovery.
2. **Safe by default.** Automation never weakens durability, authorization,
   retention, backup, update, or destructive-operation guarantees.
3. **Cross-platform by design.** Windows, macOS, Linux Desktop, and Linux
   Server are deliberate platform targets. Platform differences are isolated
   behind capability and service abstractions.
4. **Powerful when needed.** Compose, custom PostgreSQL, S3/MinIO, NAS,
   reverse proxies, custom TLS, CLI, API access, diagnostics, and filesystem
   optimizations remain available in Advanced / Server Mode.
5. **Correctness before cleverness.** This existing principle remains
   non-negotiable. Ease comes from automation and UX, never from hiding unsafe
   shortcuts.
6. **Self-hosted first.** Core data access remains useful without a mandatory
   Synveil-operated control plane or relay.
7. **Progressive disclosure.** Normal flows use user concepts; infrastructure
   detail appears under `Settings → Advanced` or administrator diagnostics.

### The terminal rule

> **A core user-facing Synveil feature should not require an ordinary user to
> open a terminal to use it.**

Exceptions may cover advanced administration, development, unsupported
environments, manual recovery, and specialized integrations. Exceptions must
not define the primary Personal / Home Mode experience.

## Two deployment experiences, one product

Personal / Home Mode and Advanced / Server Mode use the same API protocol,
domain model, PostgreSQL authority, object identity, storage correctness
rules, sync journal, backup semantics, and security model. They are packaging
and operational profiles, not incompatible products or data models.

| Profile | Primary users | Normal entry point | Infrastructure exposed by default | Advanced escape hatch |
|---|---|---|---|---|
| **Personal / Home Mode** | Non-technical users, families, desktop users, small personal servers | Native installer or guided package | Storage choice, account, device pairing, health status, safe update prompts | `Settings → Advanced`, exported diagnostics, documented migration/recovery |
| **Advanced / Server Mode** | Homelab users, NAS operators, sysadmins, VPS users, developers, larger deployments | Docker Compose, native server package, or documented manual deployment | Compose, PostgreSQL, reverse proxy, TLS, S3/MinIO, CLI/configuration | Full adapter, networking, database, and operations controls |

### Personal / Home Mode

The target flow is:

```text
native installer
    ↓
storage candidates and capacity/health check
    ↓
user confirms a data location
    ↓
Synveil-managed service and private PostgreSQL setup
    ↓
one-time account bootstrap
    ↓
local connectivity and device onboarding
    ↓
human-readable health check
    ↓
ready
```

The user should not normally configure PostgreSQL, Caddy, Docker, Compose,
TLS certificates, database roles, environment variables, mount options, or
reverse-proxy routes. The installer/service manager owns those details where
the selected platform and distribution permit it. The user still chooses or
confirms the durable data location and receives clear information about what
will and will not be removed by uninstall.

Personal / Home Mode may initially be limited to one host, one user or family
owner, one local object backend, and a supported local networking profile. A
limitation is acceptable when it is visible and safe; it is not permission to
fall back to terminal-first instructions.

### Advanced / Server Mode

Advanced / Server Mode continues to support:

- Docker Compose for homelabs, NAS systems, VPSs, developers, and operators;
- external PostgreSQL and operator-managed database backup;
- custom reverse proxies and TLS termination;
- local filesystems, NAS mounts, S3-compatible storage, and MinIO;
- manual networking, port forwarding, domains, VPN/tailnet-style access, and
  administrator-run connectivity tools;
- CLI/configuration, detailed logs, metrics, diagnostics, API access, and
  maintenance controls.

Compose remains an important production topology. It is not the only
conceptual production experience and must not be the only path represented in
product positioning or long-term architecture.

## First-class platform policy

### Host and server platforms

| Platform | Product intent | Eventual Personal / Home packaging | Advanced / Server packaging |
|---|---|---|---|
| Windows | First-class host for ordinary desktop users and advanced operators | Signed `SynveilSetup.exe`, MSI, or equivalent installer; Windows Service or a per-user/service split as appropriate | Compose on a supported runtime, native service package where practical |
| macOS | First-class host for desktop and personal-server use within Apple permissions | Signed/notarized `.dmg` or `.pkg`, launchd-managed services, explicit permission explanation | Compose or native package where practical; custom proxy/TLS remains operator-controlled |
| Linux Desktop | First-class desktop host, with distro differences documented | Native packages and/or AppImage; Flatpak only for components whose sandbox/permissions are suitable | Native package/service or Compose |
| Linux Server | First-class server host | Guided native package where practical; no consumer installer assumption | Compose, native package/service, NAS/VPS/dedicated-server deployment |

No platform is considered first-class merely because the Rust core compiles.
The support gate includes install, storage selection, service lifecycle,
health, upgrade, uninstall-with-data-preservation, backup/recovery, and the
declared filesystem/capability matrix.

### Future first-class clients

Android, iPhone, and iPad are future first-class clients. They use the same
versioned protocol and device/credential model while respecting platform
background execution, notification, permission, local-storage, and photo
library constraints. Android must be included in long-term client planning;
Apple clients must not be treated as the only mobile strategy.

### Packaging trade-offs

- Windows installers must account for service elevation, per-machine versus
  per-user data, signed binaries, Windows Defender/SmartScreen reputation,
  upgrade coordination, and uninstall prompts. A Windows Service is an
  implementation adapter, not a domain dependency.
- macOS packages must account for signing, notarization, launchd lifecycle,
  Full Disk Access or other permissions that may be needed for selected backup
  folders, Keychain access, sleep/wake, and uninstall behavior. Synveil must
  not depend on private Apple APIs.
- Linux native packages integrate well with systemd, distro paths, package
  upgrades, and administrator policy but multiply distribution maintenance.
  AppImage improves portability but has weaker integration with service
  management and privileged storage. Flatpak improves desktop isolation but
  is not automatically suitable for a privileged server, database, storage
  root, or system service. A Flatpak UI may be considered separately from the
  managed server portion.
- Linux Server packaging should provide a systemd/native route where practical
  without displacing Compose. A package is not supported until its migration,
  backup, service, permissions, and rollback behavior are tested.

These are roadmap targets. No installer is implemented by this document.

## Platform and service abstraction boundary

The domain and application core must not call Windows Service APIs, launchd,
systemd, Docker, shell commands, or platform credential APIs directly. Future
implementation uses ports/adapters conceptually similar to:

```text
PlatformRuntime
    ├── ServiceLifecycle
    ├── ProcessSupervisor
    ├── SecretStore
    ├── StorageDiscovery
    ├── NetworkDiscovery
    ├── UpdateInstaller
    └── DiagnosticsProvider
```

The platform adapter translates these capabilities to Windows Service,
launchd, systemd, a user-session supervisor, or a Compose/operator workflow.
The domain sees stable states and events, not platform-specific exit codes.

The managed service set may include:

```text
Synveil API
Synveil worker/background jobs
PostgreSQL
storage subsystem and health checks
update coordinator
logs and rotation
```

The lifecycle contract must cover install, configure, start, stop, graceful
drain, restart, crash recovery, startup ordering, health, upgrade, rollback
limits, and uninstall. A crashed optional AI or integration worker must not
restart-loop or make core storage unavailable.

## PostgreSQL without manual administration

PostgreSQL remains Synveil's canonical metadata and transactional authority as
specified by ADR-002. Personal / Home Mode does not replace it with SQLite
merely to simplify packaging and does not ask ordinary users to install roles,
create databases, set `DATABASE_URL`, or run SQL.

A future managed database deployment may provision a private PostgreSQL
distribution or supported system service, initialize a protected data
directory, create least-privilege roles, apply migrations, start/stop and
monitor the service, create coordinated backups, verify upgrades, and recover
after a crash. The database remains inside the same security and backup model
as an advanced external PostgreSQL deployment.

The managed database adapter must separate:

- provisioning and package/runtime discovery;
- database initialization and role bootstrap;
- migration execution;
- service lifecycle and health;
- backup/restore and version compatibility;
- data-directory ownership and uninstall policy;
- credential generation, storage, rotation, and recovery.

The user-facing UI reports “System database” and a health state. Administrator
diagnostics may show the PostgreSQL version and migration state. It must not
surface a stack trace or require manual database repair for ordinary flows.

## Storage selection and capability model

The first-run storage experience uses user language:

```text
Choose where Synveil should store your cloud

D:\\Synveil
3.4 TB available

[Use this location]
```

or:

```text
External SSD
1.8 TB available
Optimized for Synveil
```

The discovery layer may inspect filesystem type, capacity, writability,
removability, path safety, available capability evidence, and safely available
health information. Normal UI should not require the user to understand NTFS,
APFS, Btrfs, mount options, or object-store terminology. Detailed information
belongs under `Settings → Advanced`.

The storage abstraction is capability-based:

```text
StorageBackend
    ↓ declares
StorageCapabilities
    ↓ consumed by
Synveil storage correctness and optimization policies
```

Capabilities may include `reflink`, `block_clone`,
`copy_on_write_clone`, `native_snapshot`, `compression`, `checksumming`,
`sparse_files`, `atomic_rename`, `durable_fsync`, `range_reads`, and
`filesystem_health`. A capability is evidence from an adapter, not an
assumption from an OS name. Missing acceleration falls back to a correct
portable path or a visible unsupported state.

Synveil-native file identity, version history, sync state, backup state,
object references, change journals, conflicts, retention, integrity metadata,
deduplication metadata, and restore semantics remain independent of the host
filesystem. Btrfs snapshots are not Synveil backups; APFS snapshots are not
Synveil version history; filesystem replication is not Synveil sync.

### Filesystem support intent

The correctness model is intended to work with NTFS, ReFS, APFS, Btrfs, ext4,
XFS, later-supported ZFS, generic local filesystems, NAS-mounted filesystems,
and object stores. Btrfs may be a recommended advanced Linux accelerator. It is
not mandatory. WinBtrfs is optional/community-oriented and is never a Windows
requirement.

## Portability hazards

The platform and client contracts must explicitly test and document:

- path separators, case sensitivity/preservation, reserved names, Unicode
  normalization, component/path length, and trailing dot/space behavior;
- symlinks, junctions, hard links, permissions, ACLs, timestamps, file locks,
  atomic rename, sparse files, watchers, extended attributes, and hidden/system
  files;
- removable drives, network mounts, sleep/hibernate, process/service startup,
  user-session lifecycle, reboot, and interrupted writes;
- filesystem health signals and what happens when a device is disconnected or
  an expected storage identity changes.

Synveil preserves meaningful user metadata rather than silently normalizing it
away. A canonical internal name/comparison representation is distinct from a
platform-native presentation. An unrepresentable local name becomes an
explicit `NAME_CONFLICT`/`UNREPRESENTABLE` state with the server identity
preserved.

## Non-technical onboarding and device pairing

The desired pairing flow is:

```text
Synveil Server
    ↓ Add device
show QR code or short pairing code
    ↓
client scans/enters code
    ↓
server identity and user approval
    ↓
credential issuance and device registration
    ↓
choose sync and/or backup policies
    ↓
initial inventory and health confirmation
```

Pairing codes are short-lived, single-use or replay-resistant, rate-limited,
bound to the intended account/instance, and invalidated after approval,
expiry, cancellation, or failed-attempt limits. The design must address local
versus remote pairing, MITM protection through server identity confirmation,
lost devices, credential issuance, revocation, and audit. QR convenience never
replaces TLS or explicit user approval.

## Remote access for ordinary users

Remote access is a layered architecture problem, not a promise that one
network trick works everywhere. The planned categories are:

1. LAN discovery and direct local connections;
2. direct public connections when the operator already has safe DNS/TLS and
   firewall configuration;
3. NAT traversal where both endpoints and the network permit it;
4. optional relay/coordination service;
5. optional user-owned VPN or tailnet-style integration;
6. manual Advanced / Server Mode networking.

Personal / Home Mode should explain “remote access is ready,” “works on your
home network only,” or “needs your network administrator” in user terms. It
must not hide a failed connection behind infrastructure jargon or claim to
solve CGNAT, DNS, firewall, or certificate problems that remain unresolved.

Synveil remains useful without a proprietary Synveil-hosted service. If an
optional coordination or relay is later offered, documentation and UI must
state:

- what metadata it sees;
- whether file contents traverse it and whether content is end-to-end
  protected for that path;
- that it is optional and whether it can be self-hosted;
- failure behavior and local/manual alternatives;
- privacy, cost, abuse, availability, and data-retention implications.

The recommended direction is a self-hosted-first layered design with LAN and
manual/VPN paths always available, direct/NAT traversal where safe, and an
optional separately disclosed relay only if it materially improves adoption.
Mechanism selection remains open.

## Maintenance, health, and diagnostics

Personal / Home Mode should automate and schedule, with visible status, safe
maintenance for:

- database vacuum/maintenance and migration readiness;
- garbage collection, integrity scans, backup verification, retention cleanup;
- thumbnail/cache cleanup, certificate renewal, service recovery, and log
  rotation;
- storage health, capacity reserves, job backlog, update readiness, and device
  connectivity.

Maintenance is safe, observable, recoverable, and rate-limited. Critical work
does not silently delete recoverable data. A dry-run, plan, validation,
execution, and verification boundary remains mandatory for destructive work.

Normal UI separates:

```text
user-facing diagnostics: Storage Healthy; Database Healthy; Backups Healthy;
  Remote access Connected; Devices 4 connected
administrator diagnostics: dependency states, capacity, migration, job, and
  recovery details
developer diagnostics: correlation IDs, structured logs, traces, and redacted
  technical errors
```

The user-facing message for a full disk is “Your Synveil storage is almost
full. 182 GB remains. [Manage storage]”, while the administrator view can
retain `ENOSPC`, object IDs, and a correlation ID. Stable machine-readable API
errors remain available behind the presentation layer.

## Automatic updates

Future Personal / Home and native server installations may offer stable and
beta channels, but updates must require:

- signed releases and cryptographic verification before installation;
- service coordination, health preflight, capacity/configuration checks, and
  database migration compatibility gating;
- a verified backup before a dangerous migration or format change;
- clear notification, consent, maintenance-window, progress, and failure
  states;
- documented rollback limits, failed-update recovery, and retained prior
  artifacts/configuration where rollback is possible.

An update must never silently wipe PostgreSQL, initialize a new object root, or
claim rollback after an irreversible migration. Compose and other Advanced /
Server installations may continue to use administrator-controlled, pinned
upgrade flows. No unsigned or opaque auto-updater is part of the architecture.

## Uninstall, migration, and recovery UX

Application binaries, configuration, database state, stored user objects,
cache, logs, credentials, and backup copies have separate lifecycle labels.
The normal uninstall choices are explicit:

```text
Remove Synveil application
Keep Synveil data and configuration for later reinstall
Export or prepare migration
Permanently delete Synveil data  [destructive confirmation]
```

Uninstalling the application never implies deleting the cloud. Permanent
deletion requires explicit confirmation, a clear scope summary, a second
confirmation for irreversible data, and an audit record where the service is
still available.

Moving from an old computer to a new one should eventually follow:

```text
prepare migration
    ↓ inspect source and select destination
    ↓ validate database, object store, secrets, versions, and capacity
    ↓ copy/transfer and verify
    ↓ activate destination
    ↓ reconnect or re-register devices as required
```

The same `inspect → plan → validate → execute → verify` rule applies. Migration
must address PostgreSQL state, object references, application master keys,
device identities, hostname/TLS, remote access, temporary coexistence, rollback
window, and what happens to the old instance. A missing encryption key is
reported as a recovery blocker; a replacement key must not pretend continuity.

Recovery UX must cover an accidentally deleted file, previous version, lost
laptop, failed drive, corrupted object, broken update, database recovery, and
moving to a new server. The primary action should be understandable and
non-destructive; terminal/admin instructions are an escalation path.

## Progressive disclosure and terminology

Normal navigation and copy use:

```text
Files · Photos · Backups · Devices · Shared · Search · Activity · Settings
```

`Settings → Advanced` may reveal object store, chunk manifest, sync cursor,
garbage collection, PostgreSQL, compression profile, S3 endpoint, reflink,
storage backend, database maintenance, and detailed network controls.

Engineering identifiers remain canonical in architecture and protocol docs:
`StorageBackend`, `StorageCapabilities`, `SyncCursor`, `ChangeEvent`, and
`FileVersion` are not renamed inconsistently between languages. The user-facing
translation may say “Storage”, “Recent changes”, “Sync status”, “Version
history”, or “System database” without changing the identifier in a contract.

## Open-source and optional infrastructure

Native installers, desktop/mobile distribution, shared Rust client libraries,
SDK boundaries, Microsoft/Apple packaging, and possible App Store delivery may
affect the open-source licensing discussion in ADR-011. No license change is
made by this blueprint. Optional relay, discovery, notification, update, or
domain-assistance services must be separately identified from core self-hosted
functionality and must not become a silent SaaS dependency.

## Product-level success criteria

When the relevant capabilities are implemented and promoted, a normal user
should be able to perform these tasks without infrastructure knowledge:

- install Synveil and choose storage;
- create an account and connect another device;
- upload and sync files, enable backup, and protect mobile photos;
- restore a deleted file or previous version;
- view photos, share a file, and check system health;
- receive a verified update;
- move Synveil to another computer;
- recover after a lost device or failed server storage.

These are outcomes, not claims about the current blueprint. No fabricated
performance or adoption number is attached to them.

## Non-goals

This revision does not add a custom operating system, custom filesystem,
custom database, custom cryptography, mandatory Synveil-hosted SaaS,
Kubernetes-first deployment, or full-device OS backup. It does not make every
networking environment automatically reachable, and it does not turn native
packaging plans into implemented installers.

## Open decisions

### OPEN DECISION OD-PLAT-001: managed PostgreSQL distribution

- **Owner:** Database, Release, Security, Product
- **Needed by:** Personal / Home Mode implementation and first native installer
- **Options:** bundled/private PostgreSQL distribution; system-managed
  PostgreSQL dependency; separately packaged Synveil-managed service;
  administrator-installed external PostgreSQL only
- **Recommendation:** provide a Synveil-managed private/system service path for
  supported Personal / Home platforms while retaining external PostgreSQL in
  Advanced / Server Mode. Do not expose a database-choice wizard to ordinary
  users.
- **Decision evidence:** Windows/macOS/Linux packaging, security boundaries,
  upgrade compatibility, database backup/restore, data-directory lifecycle,
  uninstall, resource footprint, and support cost.

### OPEN DECISION OD-PLAT-002: native service supervisor boundary

- **Owner:** Platform / Distribution, Release, Security
- **Needed by:** native installer architecture gate before platform-specific
  service code
- **Options:** one privileged supervisor; per-user service plus a narrowly
  privileged helper; OS-native service per platform; user-session-only process
  model
- **Recommendation:** keep one platform-neutral `ServiceLifecycle` contract
  and use the least-privileged OS-native service model that can protect the
  data root and recover core services.
- **Decision evidence:** install elevation, IPC authentication, crash recovery,
  sleep/reboot, multi-user host behavior, update/uninstall, and threat tests.

### OPEN DECISION OD-PLAT-003: remote-access coordination mechanism

- **Owner:** Networking / Connectivity, Security, Product
- **Needed by:** remote-access beta gate
- **Options:** LAN/direct only; NAT traversal without relay; optional
  Synveil-operated relay; self-hostable relay; VPN/tailnet integration first
- **Recommendation:** ship direct/LAN/manual/VPN paths first and evaluate an
  optional, disclosed, self-hostable coordination/relay path only with a clear
  metadata/content privacy contract.
- **Decision evidence:** NAT/CGNAT matrix, threat model, failure behavior,
  privacy review, operating cost/abuse controls, and usability testing.

### OPEN DECISION OD-PLAT-004: update automation level

- **Owner:** Release, Security, Product, Operations
- **Needed by:** first stable Personal / Home release
- **Options:** notification only; opt-in download; opt-in maintenance-window
  installation; automatic security-only updates
- **Recommendation:** begin with signed notification plus explicit opt-in,
  verified backup, and migration preflight; expand only after rollback/recovery
  evidence and an air-gapped/operator policy are available.
- **Decision evidence:** update failure injection, backup/restore, migration
  compatibility, user comprehension, channel policy, and support burden.

### OPEN DECISION OD-PLAT-005: migration portability package

- **Owner:** Backup / Recovery, Database, Storage, Clients, Release
- **Needed by:** supported machine-to-machine migration gate
- **Options:** guided local transfer; portable encrypted migration archive;
  coordinated backup/restore workflow; administrator-only documented procedure
- **Recommendation:** expose a guided workflow backed by the existing verified
  backup/restore primitives; add a portable encrypted archive only when keys,
  identity, format, and resume semantics are fully specified.
- **Decision evidence:** clean destination drill, key recovery, large-data
  transfer interruption, device re-registration, hostname/remote-access change,
  and rollback tests.
