# Synveil deployment and operations architecture

Status: **PLANNED production blueprint**

Docker Compose is a supported Advanced / Server Mode production topology, not
merely a development demo. It is not the only eventual user-facing
installation. Personal / Home Mode is planned around a native/guided installer,
platform service lifecycle, storage selection, and a Synveil-managed personal
PostgreSQL deployment. Both modes use one API/domain/storage correctness model.
Kubernetes, Redis, a message broker, a service mesh and multiple network
microservices are not installation requirements.

This document defines operational contracts. It does not claim that product
deployment images, Compose files, commands, or production support already
exist. The foundation runtime implements the bounded health routes, browser
authentication transport, first-run bootstrap HTTP boundary, minimal web
setup/login/session shell documented in the API contract, and the explicit-root
local `ObjectStore` adapter/conformance boundary. The storage crate also
contains a transport-neutral owner-authorized content-read service, and the
developer API composition root wires authenticated current/historical
full/single-range download routes when `DATABASE_URL` and an explicit absolute
`SYNVEIL_OBJECT_ROOT` are both set. Authenticated version-history metadata
listing and direct lookup require the PostgreSQL metadata service but do not
require an object root or open storage. The private `synveil-worker` binary is
also implemented as an opt-in, bounded GC runtime with no listener; it is not
evidence of a deployable production image. Production download
configuration/preflight, deployment topologies, installer lifecycle, and
production support remain planned.

Prompt 37 separately implements the production HTTP `SyncRemote` library,
desktop server profiles, one-time enrollment/device bearer auth, and the
existing platform SecretStore's native persistence adapters. This is not a
shipped desktop application, installer, or new public server TLS listener.

## Prompt 37 desktop connection operations

The API binary still serves HTTP on `127.0.0.1:3000` by default. Production
deployment must put it behind a trusted HTTPS reverse proxy and keep the
cleartext API listener loopback/private; do not expose it directly to an
untrusted network. `SYNVEIL_BIND_ADDR` does not enable TLS. Set the browser's
`SYNVEIL_PUBLIC_ORIGIN` to the canonical HTTPS origin for existing CSRF checks.
The desktop profile points to that origin root, not `/api/v1` or a proxy
subpath. TLS certificate and hostname verification are mandatory; self-signed
bypass, trust-all, TOFU, and certificate pinning are not implemented.

Production profiles accept only HTTPS. A separately named non-production
constructor accepts HTTP only at literal loopback IPv4/IPv6 addresses; it is
for deterministic tests, not a deployment escape hatch. Redirects are disabled
entirely, including HTTP → HTTPS redirects: configure the final HTTPS origin
up front. No proxy environment or browser cookie jar is used by the adapter.
Finite defaults are connect 10 s, headers 20 s, metadata 30 s, stream idle
30 s, and total download 1 hour; metadata bodies are capped at 8 MiB and API
error bodies at 64 KiB. Configuration remains bounded, and automatic retries
are disabled, especially for one-time exchange.

Enrollment and recovery are explicit application operations:

1. An authenticated browser owner obtains the existing CSRF proof and POSTs a
   strict target to `/api/v1/devices/enrollment-grants`: either a new display
   name or an owned PENDING/ACTIVE Device ID. This returns one high-entropy
   enrollment token, its Device ID, and a 10-minute expiry.
2. Transfer the token privately to the desktop and exchange it once at
   `/api/v1/device-enrollment/exchange` over verified HTTPS. Do not put either
   token in command-line arguments, URLs, logs, SQLite, profile exports, or
   browser storage. No GUI, QR code, or pairing deep-link UX is supplied.
3. Store the returned bearer immediately in `PlatformRuntime::SecretStore`
   under the profile/credential identity. SQLite stores only non-secret
   profile/enrollment IDs and timestamps. Linux requires an available unlocked
   Secret Service; Windows uses Credential Manager. Other native platforms are
   explicitly unsupported at this boundary. Missing/locked/unavailable secure
   storage fails closed, without plaintext or in-memory production fallback.
4. If exchange committed but the response was lost, do not retry the grant.
   Browser owner + CSRF POSTs `{}` to
   `/api/v1/devices/{device_id}/credentials/revoke-all`, invalidating all Device
   credentials and outstanding grants while preserving its ACTIVE lifecycle.
   Create a fresh grant for the same Device and explicitly replace the local
   credential. If the credential ID is known, individual revoke is available
   at `/api/v1/devices/{device_id}/credentials/{credential_id}/revoke`.
5. Local forget/disconnect removes the local secret only. It does not imply
   server revocation while offline and does not delete replica data/progress.
   Server revoke/new enrollment is explicit; automatic rotation is deferred.

Keep `20260828000000_device_credentials_enrollment.sql` in the ordered server
migration set. It adds digest-only credentials/grants with composite owner/
Device FKs, TTL/consumption/revocation checks, and audit linkage. Grant consume,
new-device activation, and credential issuance are atomic. Credential/Device/
owner status is checked in PostgreSQL per request, so revocation takes effect
on the next request rather than after an auth cache expiry. An already running
download is not forcibly recalled. The local forward-only
`crates/client-sync/migrations/0002_server_profiles.sql` adds profile/replica
binding and non-secret enrollment/cleanup metadata without changing the
initial SQLite migration or copying any bearer secret.

The server does not expose a stable installation ID. Verified origin/TLS is
the current binding; changing a profile origin or a replica's bound profile is
not an implicit migration/re-enrollment operation. Use typed connection health
(`ONLINE`, `AUTH_REQUIRED`, `DEVICE_REVOKED`, `SERVER_UNAVAILABLE`, `TLS_ERROR`,
`PROTOCOL_ERROR`) rather than deleting local state when a connection fails.
Native SecretStore runtime evidence requires an unlocked actual OS backend;
test-store success and cross-compilation alone do not establish native parity.

| Prompt 38 capability | Status |
|---|---|
| desktop inbound sync core | `VALIDATED` |
| desktop server profiles | `IMPLEMENTED` |
| device enrollment groundwork | `IMPLEMENTED` |
| device bearer authentication | `IMPLEMENTED` |
| secure desktop credential persistence | `IMPLEMENTED` |
| production HTTP SyncRemote | `IMPLEMENTED` |
| filesystem observation | `IMPLEMENTED` |
| durable outbound intent capture | `IMPLEMENTED` |
| automatic outbound mutation submission | `NOT IMPLEMENTED` |
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop GUI/pairing UX | `NOT IMPLEMENTED` |

## Supported deployment profiles

| Profile | Purpose | Required services | Support level target |
|---|---|---|---|
| Developer | Local build/test with disposable data and explicit insecure-local exceptions | PostgreSQL, API, worker; optional Caddy/web/AI | Phase 0 developer support |
| Personal / Home | Guided/native installation for non-technical users, families, desktop users, and small personal servers | Platform runtime, Synveil API, worker, managed/private PostgreSQL, local `ObjectStore`, optional web/AI | Future first-class profile after installer/lifecycle/recovery gates |
| Advanced / Server | Homelab, NAS, sysadmin, VPS, developer, and larger self-hosted deployments | Compose/native service, operator-selected PostgreSQL, local/S3/MinIO `ObjectStore`, optional edge/AI | First advanced production target |
| Single-host local storage | Normal home server, NAS-capable host, PC or VPS with dedicated local/mounted storage | Caddy/static web, API, worker, PostgreSQL, local `ObjectStore` | Advanced / Server reference topology; not Personal / Home parity evidence |
| Single-host S3/MinIO | Metadata on the host, canonical bytes in a conformance-tested remote backend | Same core plus configured S3-compatible backend; MinIO service only if operator chooses | Later adapter promotion, not Phase 0 requirement |
| Optional local AI | Adds self-hosted model/parser runtime without changing core readiness | Core profile plus Python AI/model storage | Phase 10 optional profile |
| Optional remote AI | Uses a configured provider under explicit policy | Core plus AI dispatch/runtime and controlled egress | Phase 10 opt-in only |
| Advanced multi-node | API/worker replicas, possible broker/Kubernetes/read replicas | Only what a measured ADR approves | Future; not general early support |

A NAS-mounted directory uses the local adapter only after its atomicity,
durability, locking, case/name and error behavior pass the same capability
tests as local storage. “Can be mounted” is not evidence of a safe production
backend.

## Initial topology

```mermaid
flowchart TB
    Client["Browser and future clients"] -->|"HTTPS :443"| Caddy

    subgraph Compose["One Synveil Compose project"]
        Caddy["Caddy + static web\nonly published service"]
        API["synveil-api\nRust/Axum"]
        Worker["synveil-worker\nRust jobs"]
        Migrate["one-shot migrator"]
        PG[("PostgreSQL")]
        Objects[("Dedicated object root\ncommitted + staging namespaces")]
        AI["optional synveil-ai\nPython, no published port"]

        Caddy --> API
        API --> PG
        API --> Objects
        Worker --> PG
        Worker --> Objects
        Migrate --> PG
        Worker -. "scoped job/input" .-> AI
    end

    AI -. "explicit remote mode" .-> Provider["Allowlisted AI provider"]
    Worker -. "configured integration" .-> Forgejo["Forgejo"]
    Backup["Encrypted off-host system backup"] -. "coordinated snapshot" .-> PG
    Backup -. "coordinated snapshot" .-> Objects
```

Caddy is an edge component, not an internal domain dependency. Development and
diagnostic tests may call the API directly on a private/local bind. Product
logic does not assume Caddy-specific authentication or storage behavior.

## Personal / Home Mode lifecycle

The future native/guided flow is:

```text
installer/package
    ↓
discover safe storage candidates and capacity
    ↓
user confirms a durable location
    ↓
provision/configure Synveil-managed services and private PostgreSQL
    ↓
bootstrap account and recovery material
    ↓
start services and run bounded health checks
    ↓
pair devices and choose sync/backup policies
    ↓
ready
```

The installer hides PostgreSQL roles, `DATABASE_URL`, Compose files, reverse
proxy routes, TLS certificate mechanics, filesystem mount flags, and ordinary
environment-variable editing. It must still show the selected data location,
capacity, backup/recovery obligations, network limitations, and data-preserving
uninstall choices. A failed preflight never initializes a new empty storage
root over an existing identity.

Personal / Home Mode uses the same process responsibilities as Advanced /
Server Mode, but a platform adapter owns the service manager. Candidate
managers include Windows Service, launchd, systemd, a narrowly scoped user
service/helper, or a supported runtime supervisor. The domain core does not
know which one is active.

## Advanced / Server Mode lifecycle

Advanced / Server Mode retains the current operator workflow:

```text
choose pinned release and topology
    → configure PostgreSQL/object/secret references
    → validate paths, capabilities, ports, and capacity
    → run one migrator and start API/worker/edge
    → bootstrap and validate health
    → operate with Compose/native/CLI runbooks
```

Docker Compose remains especially important for homelabs, NAS, VPS, developers,
and administrators. It is a supported topology and a useful implementation
foundation, not the product's only conceptual deployment.

## Service lifecycle contract

The future platform/service port must cover:

| Responsibility | Required behavior |
|---|---|
| Synveil API | Start after config/schema/database/storage preflight; graceful drain; restart after bounded failure; expose health state |
| Background jobs | Start after durable job storage; lease/retry; drain without dropping work; recover after crash |
| PostgreSQL | Provision or discover according to profile; initialize least-privilege roles; start/stop/health; never reset data on ordinary failure |
| Storage subsystem | Validate identity/capabilities; reserve capacity; report missing/unavailable/removable storage; never treat a missing root as deletion |
| Update coordinator | Verify signed artifact; coordinate maintenance, backup, migration, and restart; retain failure evidence |
| Health monitor | Separate liveness, readiness, user health, administrator diagnostics, and developer evidence |
| Logs/rotation | Keep bounded local logs, preserve audit/recovery evidence, redact secrets, and rotate without deleting required evidence silently |
| Startup/shutdown | Establish dependency order, handle sleep/reboot/service termination, and resume idempotent jobs/uploads safely |

The abstraction presents stable states and actions. Windows Service, launchd,
systemd, Compose, and shell/CLI details remain adapters. No service manager
API or host process exit code belongs in the domain model.

## Managed PostgreSQL deployment model

PostgreSQL remains the canonical metadata/transaction authority. Personal / Home
Mode must conceptually support a Synveil-managed PostgreSQL lifecycle:

```text
discover/provision
→ initialize private data directory and least-privilege roles
→ apply immutable migrations
→ start and monitor
→ coordinate backup/restore
→ upgrade with compatibility gates
→ recover or enter safe maintenance on failure
```

The user sees “System database” health, not database administration. Advanced /
Server Mode can supply external PostgreSQL, custom backup, TLS, connection pool,
and migration operations. There is no dual SQLite/PostgreSQL product contract.

Remaining feasibility questions include bundled/private distribution, system
PostgreSQL, packaged service dependency, upgrade compatibility, PostgreSQL
backup/restore, Windows/macOS support, data-directory ownership, uninstall,
resource footprint, and security boundaries. These remain `OPEN DECISION` items
in [PLATFORM.md](PLATFORM.md) and ADR-019.

## Service responsibilities and privileges

| Service | Responsibility | Network/volume access | Must not have |
|---|---|---|---|
| `caddy` | TLS termination, canonical host, routing, static hashed web assets, edge time/size/connection limits | Published 80/443; private route to API; Caddy config/certificate state | PostgreSQL/object/AI/Git credentials, Docker socket |
| `synveil-api` | Authentication, authorization, bounded request parsing/streaming, application commands/queries, stable errors | Private PostgreSQL; configured object root/backend; no public bind except through Caddy | Migration privilege, Docker socket, model/runtime admin, unrestricted host filesystem |
| `synveil-worker` | Claim PostgreSQL jobs/outbox, reconciliation, integrity, retention/GC, derivatives and integrations | Private PostgreSQL; object root/backend; optional AI/approved egress | Published port, schema-owner privilege, Caddy/host credentials |
| `migrate` | Validate and apply one ordered migration set under advisory lock | PostgreSQL with migration role; immutable migration files | Long-running service, object root, public port |
| `postgres` | Authoritative metadata, journals, jobs, audit and idempotency | Private core network; dedicated persistent data volume | Internet-published port in production |
| `synveil-ai` | Optional bounded OCR/model/embedding computation and version-bound derived output | Private AI boundary; scoped job/input/output path; explicit provider egress only in remote mode | Public port, broad administrator credential, canonical delete, unrelated database/object access |
| Optional MinIO | Operator-selected S3-compatible backend after adapter conformance | Private storage network and dedicated persistent data | Public administrative console by default, shared root credential in API logs/config |

API and worker may use the same immutable image with distinct commands and
resource limits. This preserves one code/version set without combining process
failure domains.

## Networks, ports and outbound access

### Inbound

- Only Caddy publishes production ports, normally TCP 80 for ACME/redirect and
  443 for HTTPS. Operators may use DNS challenge or an already managed
  certificate where inbound 80 is not available.
- PostgreSQL, API, worker, AI and MinIO/admin endpoints use internal networks or
  loopback-only diagnostic binds. A firewall/host scan verifies they are not
  externally reachable.
- Metrics and detailed health are internal or separately authenticated. A
  public liveness response contains no dependency, version, path or capacity
  detail useful for reconnaissance.

### Internal networks

Use distinct logical networks where Compose/runtime support makes the boundary
meaningful:

- `edge`: Caddy to API;
- `core`: API/worker/migrator to PostgreSQL;
- `ai`: Rust worker/API capability channel to optional AI;
- `storage`: only when a network object store is configured.

Network separation supplements, but does not replace, application
authorization and scoped credentials.

### Outbound

Core local-storage operation requires no vendor control plane or telemetry
egress. Outbound destinations are purpose-specific:

- ACME/DNS provider only when Caddy certificate configuration requires it;
- explicitly configured Forgejo origin;
- explicit remote AI provider only in `REMOTE` mode;
- operator-invoked image/model/update acquisition.

Compose networks alone are not a complete egress firewall. Production guidance
must show how to enforce allowlists with host firewall/proxy controls where the
operator requires them. Redirect/DNS/private-address rules follow
[SECURITY.md](SECURITY.md).

## Persistent state and filesystem layout

Logical volume names are illustrative; packaged deployment may map them to
validated bind paths.

| State | Persistence and backup rule |
|---|---|
| PostgreSQL data | Dedicated volume/path, owned only by PostgreSQL; never share with object bytes. Back up logically or with a reviewed database snapshot method. |
| Object root | Dedicated storage identity containing separate generated `staging/` and immutable committed namespaces on the same durability domain when local atomic promotion is used. No user path mapping. |
| Caddy state | Certificate/account state and configuration; certificate state can often be recreated, but custom CA/DNS credentials are secrets and need operator recovery. |
| Configuration | Versioned non-secret file plus deployment-specific overrides. Back up the effective redacted configuration and schema version. |
| Secrets | Host-protected mounted files or external secret references, separate from source/config; secure off-host recovery is mandatory for master keys. |
| AI models/cache | Optional, bounded and replaceable unless license/download availability requires preservation. Never the only copy of canonical user data. |
| Parser/job temporary data | Bounded writable temp area; disposable after process crash and reconciled by job identity. |
| System backups | Separate failure domain/off-host destination, not another directory on the same disk presented as protection. |

Repository status: the explicit-root local filesystem adapter and its managed
`objects/` plus `staging/` layout are `IMPLEMENTED/VALIDATED` at the storage
crate boundary. The developer API composition root accepts
`SYNVEIL_OBJECT_ROOT` only alongside `DATABASE_URL` and then installs the
validated upload and download application services behind the authenticated HTTP
routes; an unset root fails closed rather than selecting a default. The storage
crate's content-read application service and API download transport use the same
metadata/`ObjectStore` ports. Safe version restore is implemented at the
authenticated API/metadata boundary but still requires the PostgreSQL metadata
service. With `DATABASE_URL`, the API also requires
`SYNVEIL_REBASELINE_TOKEN_KEY` as exactly 64 lowercase hexadecimal characters
(32 random bytes). It is a deployment secret: keep the same value across
process restarts and replicas, inject it through the supported secret boundary,
and never print it or commit it. Missing/malformed configuration fails startup
closed; rotating it deliberately invalidates outstanding bootstrap cursor and
completion tokens, so OPEN sessions must restart. HTTP download production
configuration/preflight, automatic conflict resolution, backup, and installer
wiring remain `PLANNED`; typed client mutation submission, per-device
feed/checkpoint validation, and the server-side logical rebaseline bootstrap
are implemented/validated at their respective boundaries. Metadata-only GC
planning and the internal physical execution service are
`IMPLEMENTED/VALIDATED`, while the private bounded GC worker is `IMPLEMENTED`.
Version-history metadata itself remains available from the configured metadata
service without object-root setup.

### Local object-root validation

The implemented `LocalFilesystemObjectStore::open` boundary verifies:

- the resolved path is non-empty and absolute, has directory type, is not a
  filesystem root, user home/profile, current directory, build-time source
  workspace, symlink, junction, or other reparse-point root;
- the versioned local-layout marker has the expected bounded content;
- required `staging/` and `objects/v1/` namespaces exist directly below the
  root and are not redirected entries; and
- file sync, directory sync, same-root hard-link promotion, and rename behavior
  are probed to populate capabilities without inferring from an OS name.

Runtime configuration/readiness has not yet been wired. Ownership/mode policy,
PostgreSQL-directory exclusion, persisted `StorageBackend` identity binding,
free-space/inode reserves, mount replacement detection, and an operator-facing
health report remain `PLANNED`; deployment must not claim those checks are
active merely because the adapter can be constructed.

The explicit open routine may create a missing configured directory, but it
rejects an unexpected marker or redirected managed entry. Higher-level runtime
composition must decide when a missing previously bound root fails readiness;
it must never interpret that condition as “all objects were deleted,” and a
changed path string is never an object-store migration.

### Staging placement

For the local adapter, upload staging used for atomic promotion must reside on
the same filesystem/durability domain as the committed namespace unless the
application uses a tested copy-and-verify finalization path. A convenient
separate temporary volume must not silently turn rename into a cross-device
copy. Staging has independent accounting/expiry even when it shares the object
root.

## Linux service identity and filesystem ownership (Prompt 75)

Prompt 75 establishes the Gen-1 least-privilege Linux service identity and
ownership contract required by the Prompt 71–74 scheduled-maintenance
deployment.

### Service account

| Property | Value | Notes |
|---|---|---|
| Account name | `synveil` | persistent system user |
| Group name | `synveil` | persistent system group |
| Type | system account | `systemd-sysusers` declarative, not `useradd` shell logic |
| Login | disabled | invalid password; no interactive shell |
| Shell | `/usr/sbin/nologin` (or distribution `nologin` equivalent) | `PrivateTmp`, `ProtectHome` etc. remain |
| Home | `/var/lib/synveil` | stable state location, **not** `/home/*`; no conventional interactive home |
| UID/GID | auto-allocated (`-` in `sysusers`) | **no hardcoded numeric UID**; future appliance images may reserve a fixed value, but Gen-1 assumes nothing cross-machine |
| Supplementary groups | **none** | no `sudo`/`wheel`/`docker`/`disk`/`adm`/`root` |

Declarative sources (packaging installs to `/usr/lib/...`, overrides in
`/etc/...`):

- `deploy/sysusers.d/synveil.conf` — `g synveil -` + `u synveil - "Synveil
  service account" /var/lib/synveil /usr/sbin/nologin`
- `deploy/tmpfiles.d/synveil.conf` — `d /var/lib/synveil 0750 synveil synveil - -`
- `deploy/systemd/synveil-scheduled-maintenance.service` —
  `User=synveil`, `Group=synveil`, `RuntimeDirectory=synveil`,
  `RuntimeDirectoryMode=0750`

Creation is a **packaging/deployment** responsibility. The Rust runtime
(`synveil-scheduled-maintenance-once → DatabasePool →
ScheduledMaintenanceCycleRunner`) never creates system users, never `chmod`s
system directories, never invokes `systemctl`, and never escalates.

### Why runtime does not use `root`

The one-shot requires only PostgreSQL connectivity (TCP `AF_INET`/`AF_INET6`
or `AF_UNIX` Unix socket). It never needs to modify its own binary, unit
files, host configuration, or storage roots. Running as `root` would violate
least privilege and allow an exploited process to replace its executable.
Package **install** remains `root`-owned; **runtime** is unprivileged.

### Why `DynamicUser=yes` is rejected

| Requirement | `DynamicUser` behavior | Consequence for Synveil |
|---|---|---|
| Stable ownership of `/var/lib/synveil` | Allocates ephemeral UID per activation, hides real path under `/var/lib/private` via id-mapped mounts | Administrator cannot see/persist/inspect state; upgrades and multiple services cannot share ownership |
| Access to `/etc/synveil` config | Transient user not in predictable group; permissions would need world-readable or per-activation ACL | Secrets would leak or access would break |
| Future object-store / data paths | Same private-mount hiding | Storage ownership becomes non-durable |
| Multiple first-party services sharing state | Each activation gets a different UID | No stable identity for co-owned state |

`DynamicUser` is therefore suitable only for fully stateless, ephemeral
services. The repository records the rejection; a future storage
architecture that proves no stable filesystem ownership is required would
trigger a STOP-and-report rather than forcing the persistent decision.

### Filesystem ownership contract

| Path | Ownership | Mode | Created by | Runtime access |
|---|---|---|---|---|
| `/usr/bin/synveil-*` | `root:root` | `0755` | package (root) | `synveil` can read/exec, **not write** — service cannot modify its own executable |
| `/usr/lib/systemd/system/synveil-*.service` `/usr/lib/systemd/system/synveil-*.timer` | `root:root` | `0644` | package | not writable by `synveil` |
| `/usr/lib/sysusers.d/synveil.conf` `/usr/lib/tmpfiles.d/synveil.conf` | `root:root` | `0644` | package | not writable |
| `/etc/synveil` | `root:synveil` | `0750` | package | `synveil` reads required files (via `EnvironmentFile`), cannot freely rewrite admin config |
| `/etc/synveil/synveil-scheduled-maintenance.env` | `root:synveil` | `0640` | admin / package | `synveil` reads optional non-secret `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS`; `DATABASE_URL` is delivered via Prompt 77 `LoadCredential`, not this file |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | `tmpfiles.d` | persistent non-secret state; future appliance bookkeeping; empty today is acceptable, not in `/usr`/`/etc`/home |
| `/run/synveil` | `synveil:synveil` | `0750` | `RuntimeDirectory=` | ephemeral per-activation, lifecycle-tied cleanup, never manually persisted |
| `/var/log/synveil` | — | — | — | **intentionally not created** — journald is preferred; only add if Synveil actually writes log files |

**Storage / object-store boundary:** user storage pools obey the storage
architecture. Prompt 75 does **not** recursively `chown` arbitrary pools to
`synveil`; only required service-state boundaries are documented.

### Runtime/state directory decision

- `/run/synveil` → `RuntimeDirectory=synveil` (not `tmpfiles.d`). Correct
  ownership, automatic cleanup, no stale host state, single manager.
- `/var/lib/synveil` → `tmpfiles.d` (`d` line) at boot/package time, **not**
  `StateDirectory=` per-service. One host-level declaration remains
  authoritative for all future Synveil services sharing the directory; per-service
  `StateDirectory=synveil` would create conflicting managers for the same path.
- `/etc/synveil` → package creates with `root:synveil 0750`; not managed via
  `tmpfiles.d`.
- No secret files are created via `tmpfiles.d`.

### Privilege boundary

| Operation | Required privilege |
|---|---|
| Install units/sysusers/tmpfiles/binaries/config skeleton | `root` (package) |
| `systemd-sysusers`, `systemd-tmpfiles --create`, `daemon-reload`, `systemctl enable` | `root` |
| `install -m 0640 -o root -g synveil /etc/synveil/synveil-scheduled-maintenance.env` (non-secret tuning) | `root` |
| `install -m 0600 -o root -g root /etc/synveil/credentials/database-url` (database secret) | `root` |
| One-shot execution (`synveil-scheduled-maintenance-once`) | `synveil` (TCP/`AF_UNIX` to PostgreSQL only) |
| Modifying binaries/units/`/etc/synveil` globally | denied to `synveil` |

The one-shot binary has **zero** `UID 0` dependency on its normal
database-only path; it runs under the already-unprivileged test user in CI
to prove rootless operation.

### Database connectivity

Running as `synveil` remains compatible with the current `DATABASE_URL`
architecture (TCP or Unix socket). No PostgreSQL `peer` authentication tied to
the Unix username is assumed; connectivity is an administrator/deployment
choice. Authentication is not weakened.

### Secret boundary

Prompt 75 establishes the filesystem ownership baseline. Prompt 77 now owns
database-secret delivery: the ordinary environment file is non-secret tuning
only, while the administrator-controlled credential source is
`root:root 0600` and delivered through `LoadCredential`.

### Per-service account evaluation

Future identities such as `synveil-api`, `synveil-maintenance`, `synveil-gc`
were evaluated. All first-party services currently share a trusted
backend/data boundary; separate accounts would add packaging complexity
without materially improving least privilege. **Recommendation Gen-1:** one
`synveil` account. The decision is recorded in ADR and `BACKUP.md`; a later
capability separation may justify per-service accounts.

### Portability

Linux `User=`/`Group=`/`UID`/`GID`/systemd remain outside `crates/core` and
portable Synveil Server APIs. No domain model gains a Linux identity field.

## Linux package lifecycle and data-preserving uninstall (Prompt 76)

Prompt 76 is the package-neutral installation foundation for the Gen-1
scheduled-maintenance deployment. It defines deterministic mechanics for fresh
install, idempotent reinstall, in-place upgrade, failed-upgrade recovery,
uninstall, uninstall-with-preservation, explicit purge, `root` vs
`synveil:synveil` ownership, `systemd`/`sysusers`/`tmpfiles` integration, and
safe staged-root testing without host mutation. No distribution-specific
`.deb`/`.rpm`/PKGBUILD/OCI/Synveil OS image is created yet — future packaging
calls this layer.

### Authoritative manifest

`deploy/install/MANIFEST` is the single source of truth. All install/remove
mechanics read it; no duplicated destination logic exists in `install.sh` or
`uninstall.sh`.

```
source                              destination                                      mode  owner   group    class
BINARY                              /usr/bin/synveil-scheduled-maintenance-once      0755  root    root     PACKAGE
deploy/systemd/*.service            /usr/lib/systemd/system/synveil-*.service        0644  root    root     PACKAGE
deploy/systemd/*.timer              /usr/lib/systemd/system/synveil-*.timer          0644  root    root     PACKAGE
deploy/sysusers.d/synveil.conf      /usr/lib/sysusers.d/synveil.conf                 0644  root    root     PACKAGE
deploy/tmpfiles.d/synveil.conf      /usr/lib/tmpfiles.d/synveil.conf                 0644  root    root     PACKAGE
deploy/config/*.env.example         /usr/share/synveil/*.env.example                 0644  root    root     PACKAGE
-                                   /etc/synveil                                     0750  root    synveil  CONFIG_DIRECTORY
-                                   /var/lib/synveil                                 0750  synveil synveil  STATE_DIRECTORY
-                                   /run/synveil                                     0750  synveil synveil  RUNTIME_MANAGED
```

`PACKAGE` is `root:root` immutable payload replaced on upgrade and removed on
uninstall. `CONFIG_DIRECTORY` is `root:synveil` skeleton; `STATE_DIRECTORY` and
`RUNTIME_MANAGED` are `synveil:synveil` managed by `tmpfiles.d` /
`RuntimeDirectory=` (not seeded by payload). No `/var/log/synveil` is created
(journald).

### Package-neutral interface

```
deploy/install/install.sh   --root=<staged-root> [--binary=<path>]  [--destdir=<root>]
deploy/install/uninstall.sh --root=<staged-root> [--purge]
deploy/install/common.sh    # shared helpers (sourced, not executed)
deploy/install/MANIFEST     # authoritative
```

All mechanics support an alternate root (`DESTDIR`):

```
DESTDIR=/tmp/synveil-root ./deploy/install/install.sh --binary=target/debug/synveil-scheduled-maintenance-once
./deploy/install/install.sh --root=/tmp/synveil-root --binary=...
```

Tests operate on disposable `/tmp/.../root` trees and never write to
`/usr`/`/etc`/`/var`/`/run` on the developer host unless inside a disposable
container.

### File replacement safety

`install.sh` never truncates an existing executable/unit in place. It writes
the new content to a temporary file (`cp` → `chmod` → `chown` if root →
`mv -f` atomic rename). `uninstall.sh` unlinks known `PACKAGE` paths with
`rm -f` without following symlink targets.

### Fresh install

A staged fresh install produces:

- `PACKAGE` files at the five `root:root` destinations plus the template at
  `/usr/share/synveil/...example` (`0644`).
- `CONFIG_DIRECTORY` `/etc/synveil` (`0750 root:synveil`, empty).
- No `/var/lib/synveil` seed, no `/run/synveil`, no `env` file.
- No unexpected files.

The binary is `synveil-scheduled-maintenance-once` built from
`crates/api/src/bin/synveil-scheduled-maintenance-once.rs`; tests may use a
controlled fixture for path/mode mechanics, but the contract references the real
artifact.

### Idempotent reinstall

Running `install.sh` twice on the same staged root succeeds, does not duplicate,
does not corrupt modes, does not overwrite `admin-owned` config, and leaves
`PACKAGE` checksums identical. This is verified by snapshot/hash manifest in
`linux_install_lifecycle.rs`.

### Upgrade

```
Version N  →  Version N+1
  binary/units/sysusers/tmpfiles may change
  /etc/synveil + /var/lib/synveil + external pools are preserved
```

Upgrade atomically (per-file) replaces `PACKAGE` files while preserving
administrator and runtime data. No transactional package-manager semantics are
claimed at this layer.

### Failed-upgrade recovery

`SYNVEIL_INSTALL_FAIL_AFTER=N` (test-only) simulates failure after N `PACKAGE`
artifacts. The harness proves:

- `admin config` (`/etc/synveil/*.env`) remains byte-identical;
- `persistent state` (`/var/lib/synveil/*`) remains untouched;
- `external user-data` outside package lifecycle remains untouched;
- `PACKAGE` version may be **partially updated** (already-replaced files stay at
  new version).

Data preservation is **LOCKED**; package-version rollback is **DEFERRED** to
distro packaging. See ADR-024.

### Ordinary uninstall

```
./deploy/install/uninstall.sh --root=/tmp/root   # default
```

Removes only `PACKAGE` artifacts:

- `/usr/bin/synveil-scheduled-maintenance-once`
- `/usr/lib/systemd/system/synveil-scheduled-maintenance.service|.timer`
- `/usr/lib/sysusers.d/synveil.conf`
- `/usr/lib/tmpfiles.d/synveil.conf`
- `/usr/share/synveil/*.example`

Preserves:

- `/etc/synveil` and `/etc/synveil/*.env` (admin config)
- `/var/lib/synveil` (persistent state)
- external storage pools, backup destinations, object-store roots, home data
- shared parent directories (`/usr/bin`, `/usr/lib/...`, `/etc`, `/var/lib`)
- PostgreSQL data and rows
- `synveil` system account (avoids orphaned UID; reinstall is safe)

Reinstall after ordinary uninstall restores `PACKAGE` files and reuses preserved
state — the `replace system software without destroying persistent data`
principle.

### Purge

Separate, destructive, **explicit** flag:

```
./deploy/install/uninstall.sh --root=/tmp/root --purge
```

With `--purge`, after removing `PACKAGE` files, it also recursively removes
`/etc/synveil` and `/var/lib/synveil` after allowlist + lexical containment
(`realpath -m -s`) + symlink-unlink checks. It still **never** deletes
external pools, backup destinations, object-store roots, mounted volumes, home
data, or PostgreSQL. No `purge` is inferred from a plain package-manager
`uninstall`; the flag is required. The `synveil` account is retained even on
purge (admin may manually `userdel` after confirming no orphaned files).

### Configuration installation policy

Prompt 77 owns final secret delivery. Fresh install **does not** create a
working `DATABASE_URL` file (policy **A: no file**). It:

1. Creates `/etc/synveil` (`0750 root:synveil`);
2. Installs an example template at `/usr/share/synveil/
   synveil-scheduled-maintenance.env.example` (`0644 root:root`) that contains
   no credentials (comments + optional non-secret lease setting).

Admin creates the real env via:

```
sudo install -d -m 0750 -o root -g synveil /etc/synveil
sudo install -m 0640 -o root -g synveil \
  /usr/share/synveil/synveil-scheduled-maintenance.env.example \
  /etc/synveil/synveil-scheduled-maintenance.env
# edit only non-secret lease tuning; provision DATABASE_URL through the
# root-owned /etc/synveil/credentials/database-url file
```

`EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env` (note `-`)
means the service handles absence gracefully; no placeholder points to
production or insecure defaults.

### Service user on uninstall

- **Ordinary uninstall**: retains `synveil` account while any Synveil-owned
  files/state may remain (prevents orphaned ownership ambiguous on reinstall).
- **Purge**: may be safe to remove `synveil` only after explicit purge and
  administrator confirmation that no `synveil`-owned files remain. The script
  does **not** auto-`userdel`; it logs the manual step.

Documented in `uninstall.sh`.

### `systemd` lifecycle ordering (real host, not staged test)

```
install PACKAGE files
  → systemd-sysusers               # reads /usr/lib/sysusers.d/synveil.conf
  → systemd-tmpfiles --create      # creates /var/lib/synveil
  → systemctl daemon-reload
  → systemctl enable synveil-scheduled-maintenance.timer  # explicit; not auto-started by package-neutral layer
```

This layer logs but does not execute those host commands during staged tests.
Enablement is explicit; installation makes the timer available but does not
silently start scheduled maintenance.

Uninstall ordering (real host):

```
systemctl disable --now synveil-scheduled-maintenance.timer
systemctl stop synveil-scheduled-maintenance.service  # allow bounded completion; don't kill healthy cycle
<remove PACKAGE via packaging or uninstall.sh --root=/>
systemctl daemon-reload
```

### Upgrade while service is active

A bounded one-shot (`CREATED→SNAPSHOT_CAPTURED→EXPIRY_PLANNED→COMPLETED`)
continues to completion using its already-loaded process image even while
`/usr/bin/synveil-scheduled-maintenance-once` is atomically replaced. The next
timer activation uses the new binary. No kill is issued merely to replace the
executable.

### Path, symlink, and parent safety

- Every helper validates `STAGED_ROOT` is absolute, non-empty, not `/` without
  `SYNVEIL_ALLOW_HOST_ROOT=1`, no `..` component, not a symlink.
- Every destination is validated to stay under `STAGED_ROOT` via
  `realpath -m -s` (lexical) plus parent-dir realpath check (catches
  `mkdir -p` parent symlink escape).
- Uninstall unlinks known `PACKAGE` paths without following symlink targets;
  purge unlinks symlink `CONFIG_DIRECTORY`/`STATE_DIRECTORY` without traversing
  the target. Tests inject malicious symlinks pointing outside the staged root.
- No shared parent (`/usr/bin`, `/usr/lib/...`, `/etc`, `/var/lib`) is
  `rm -rf`'d; only known Synveil leaves are removed.

### Shell safety

`set -euo pipefail`, all paths quoted, no `eval`, no `curl|sh`, no unquoted
globs, `rm -rf "$full"` always validated and scoped to manifest allowlist.
Checked via `bash -n` and `linux_install_lifecycle::shell_safety_set_euo…`.

### Tests

`crates/metadata/tests/linux_install_lifecycle.rs` provides 33 focused tests
covering the 18 required behaviors plus credential lifecycle (fresh/reinstall/
upgrade/uninstall/purge/symlink/rotation), permission, manifest, shell, and
database-preservation invariants. All use temporary roots; no `systemctl
enable`, `useradd`, `chown -R` on host, or `systemd-sysusers/tmpfiles` against
host root. `crates/api/src/runtime_database_credential.rs` provides 15 unit
tests for file/ENV precedence, missing/empty/oversized/malformed/trailing-
newline/dual-source/secret-not-logged cases.

## Linux runtime configuration & secure credential delivery (Prompt 77)

Prompt 77 finalizes the Gen-1 Linux runtime configuration and secret-delivery
architecture. The transitional `DATABASE_URL` in `EnvironmentFile` is replaced
for production systemd deployment by `systemd` `LoadCredential=` (least privilege
without world-readable or `synveil`-readable admin source).

### Architecture

```
administrator-owned secret source (/etc/synveil/credentials/database-url, root:root 0600)
        ↓
systemd LoadCredential=database-url:/etc/synveil/credentials/database-url
        ↓
per-service credential directory ($CREDENTIALS_DIRECTORY/database-url, 0400, unswapped, read-only)
        ↓
Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url  (non-secret path)
        ↓
one-shot Synveil runtime reads file once (bounded 8 KiB, trims single trailing \n/CRLF)
        ↓
DatabaseConfig::from_url(...)  (canonical validation)
        ↓
DatabasePool::connect → MigrationRunner → ScheduledMaintenanceCycleRunner (exactly once)
```

`crates/core` remains free of systemd concepts; `crates/api/src/
runtime_database_credential.rs` is the narrow runtime-edge helper.
`LoadCredential`, `CREDENTIALS_DIRECTORY`, `/etc` are not imported into portable
domain.

### Secret source ownership

| Path | Owner | Mode | Access |
|---|---|---|---|
| `/etc/synveil/credentials` | `root:root` | `0700` | `CREDENTIAL_DIRECTORY` in `MANIFEST`; created as skeleton, never seeds secret |
| `/etc/synveil/credentials/database-url` | `root:root` | `0600` | administrator source; `synveil` has **no direct read** — systemd reads as PID 1 |
| `/run/credentials/synveil-scheduled-maintenance.service/database-url` or `$CREDENTIALS_DIRECTORY/database-url` | systemd, `root:synveil` effective | `0400` | per-service credential copy; read-only, unswapped, only `synveil` (and root) can read |

World-readable (`0644`/`0666`) is forbidden. `root:synveil 0640` (old transitional)
is replaced by `root:root 0600/0700` for the actual database secret — stronger
boundary than the prior `0640` model.

### EnvironmentFile role after Prompt 77

`EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env` remains for
**non-secret** tuning only:

```
# non-secret example
SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS=120
```

It **must not** contain `DATABASE_URL`, password, token, or private key in the
production example. The service no longer requires `DATABASE_URL` in that file;
`LoadCredential` is authoritative. The template at
`deploy/config/synveil-scheduled-maintenance.env.example` contains no
`DATABASE_URL=postgresql://` line with credentials (only comments and lease
example), validated via `production_env_example_contains_no_database_url_secret`.

### Config classification

| Class | Examples | Delivery |
|---|---|---|
| `NON-SECRET` | lease duration `10..=900`, future cadence tuning, diagnostic level | `EnvironmentFile` (`/etc/synveil/*.env` `0640 root:synveil`) |
| `SECRET` | `DATABASE_URL` (with password), future encryption/service tokens, private keys | `systemd` `LoadCredential` → `$CREDENTIALS_DIRECTORY` file, or secure secret-file mechanism |

### Rust boundary

The one-shot now does:

```rust
let database_config = database_config_from_runtime()?; // credential file or DATABASE_URL fallback
// load_database_url_from_runtime_source() → bounded file read / env fallback → DatabaseConfig::from_url
let pool = DatabasePool::connect(&database_config).await?;
```

No duplicated URL parsing; `DatabaseConfig::from_url` remains canonical validator.
The helper is generic file-based (`load_database_url_from_file`) not systemd-
deep; `crates/metadata` is not polluted.

### Credential path mechanism

The service sets:

```
LoadCredential=database-url:/etc/synveil/credentials/database-url
Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url
```

`%d` is the systemd credentials-directory specifier (systemd 261, `man
systemd.exec` `CREDENTIALS` / `man systemd.unit` `%d`). At runtime it expands
to `$CREDENTIALS_DIRECTORY` (e.g. `/run/credentials/synveil-scheduled-
maintenance.service`). The helper prefers `SYNVEIL_DATABASE_CREDENTIAL_FILE`
explicit path, else `$CREDENTIALS_DIRECTORY/database-url`. This supports both
the `%d` specifier and direct `$CREDENTIALS_DIRECTORY` use. The secret itself is
never copied into an environment variable.

### Development / non-systemd compatibility

For developers, tests, manual invocation:

```
DATABASE_URL=postgresql://dev:dev@127.0.0.1:5432/synveil \
  cargo run -p synveil-api --bin synveil-scheduled-maintenance-once
```

remains supported as a **fallback** when no credential file is configured.
Symmetric for manual file use:

```
SYNVEIL_DATABASE_CREDENTIAL_FILE=/tmp/my-creds/database-url \
  cargo run -p synveil-api --bin synveil-scheduled-maintenance-once
```

### Source precedence

1. Explicitly delivered credential file (`SYNVEIL_DATABASE_CREDENTIAL_FILE` or
   `$CREDENTIALS_DIRECTORY/database-url`) — authoritative if present
2. Development fallback `DATABASE_URL` — only if no credential file source is
   configured

**Do NOT silently combine.** If both a credential file path and `DATABASE_URL`
are set, the runtime **fails closed** with `database credential is ambiguous:
both credential file and DATABASE_URL are set; use only one` (no secret in
message). This is tested via `dual_source_is_ambiguous` and documented as the
chosen policy **B** (reject ambiguous). `SYNVEIL_DATABASE_CREDENTIAL_FILE`
empty after trim is treated as not set, allowing fallback; whitespace-only
credential file fails as empty.

### Missing / empty / oversized / malformed semantics

- **Missing credential** (no file path and no `DATABASE_URL`): `database
  credential is missing` → non-zero exit before any scheduler tick or worker
  step, no DB connection, no default `postgres://localhost/default`.
- **Unreadable file** (`open` fails, directory): `database credential file is
  unreadable`.
- **Empty or whitespace-only** (after trimming single trailing `\n`/`\r\n`):
  `database credential is empty`.
- **Oversized** (`> 8 KiB`): `database credential exceeds maximum size (8192
  bytes)` — checked via `metadata.len` fast path and after read.
- **Malformed URL** (fails `DatabaseConfig::from_url`): `database configuration
  is invalid: database URL must use the PostgreSQL scheme` etc. No fix-up.

All errors are generic and contain no credential value. Trailing single
`\n` or `\r\n` is stripped (common for admin-managed secret files); interior
content is not rewritten; arbitrary spaces are not trimmed.

### Credential file size bound

`MAX_CREDENTIAL_FILE_SIZE = 8 * 1024` (8 KiB). A `DATABASE_URL` is
typically < 500 bytes even with long hosts; 8 KiB is ample margin without
streaming complexity. Reported on oversized.

### Secret logging rule

Never logged: `DATABASE_URL`, password, file contents. Allowed diagnostics:
`source type = systemd credential` vs `environment fallback`, `credential path
class` is not emitted (path could reveal deployment). Errors are `database
credential is ...` without value. Verified via `no_secret_logging_in_errors`
that oversized/malformed errors do not contain `postgresql://`.

Error messages are exactly the strings above; no credential value is
interpolated.

### systemd unit integration

`deploy/systemd/synveil-scheduled-maintenance.service` now contains:

```
LoadCredential=database-url:/etc/synveil/credentials/database-url
Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url
EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env
```

Preserves `Type=oneshot`, `User=synveil`, `Group=synveil`,
`NoNewPrivileges=yes`, `RuntimeDirectory=synveil`, `Restart=no`,
`RestrictAddressFamilies`, etc. Validated via `systemd-analyze verify`
(patched `ExecStart=/usr/bin/true` for isolated syntax check). Credential
directives parse cleanly on systemd 261.

### Installer manifest integration

`deploy/install/MANIFEST` adds:

```
-  /etc/synveil/credentials  0700  root  root  CREDENTIAL_DIRECTORY
```

Package **does not** bundle a secret file. Fresh install creates/decalares
both `/etc/synveil` (`0750 root:synveil`) and `/etc/synveil/credentials`
(`0700 root:root`) but never invents `database-url`. `install.sh` handles
`CREDENTIAL_DIRECTORY` like `CONFIG_DIRECTORY` (mkdir + chmod/chown if root,
idempotent). Reinstall/upgrade preserve `database-url` byte-identical via
directory preservation (no file creation). Ordinary uninstall preserves
`/etc/synveil` and `/etc/synveil/credentials/database-url`; explicit
`--purge` may remove `/etc/synveil` (including `credentials/*`). Symlink-safe
(`realpath -m -s` lexical + parent realpath, `rm -f` without follow).

### Credential template

No real secret is shipped. `deploy/config/synveil-scheduled-maintenance.env.
example` now documents `LoadCredential` and contains no
`DATABASE_URL=postgres(at)://` functional line (only non-secret lease). If an
example is needed, instructions are given not an installed secret.

### Fresh install / reinstall / upgrade / uninstall / purge

- **Fresh**: creates `/etc/synveil` + `/etc/synveil/credentials` (correct modes),
  no `database-url` secret; activation fails safely with missing-credential
  before DB/migration.
- **Reinstall**: preserves `credentials/database-url` byte-identical (no overwrite)
- **Upgrade**: preserves contents + permissions; PACKAGE artifacts may change
- **Ordinary uninstall**: preserves `/etc/synveil`, `/etc/synveil/credentials`,
  `database-url`, `/var/lib/synveil` (Prompt 76 contract)
- **Purge** (`--purge`): may remove `/etc/synveil` including `credentials/*`
  (explicit, destructive); still never deletes external pools/DB.

Symlink safety extends to `credentials` dir/file (inject malicious symlink to
external pool, prove uninstall/purge only unlinks, not traverses). Verified via
`credential_symlink_safety_does_not_follow_external`.

### Secret rotation

Admin atomically replaces secret outside application:

```
write new secret to adjacent root-owned temp file (0600) + atomic rename
# e.g. install -m 0600 -o root -g root /tmp/new-url /etc/synveil/credentials/database-url
```

Next one-shot activation receives new credential via `LoadCredential`. No
long-running daemon restart, no watcher, no polling. Verified via
`credential_rotation_atomic_new_invocation_reads_new_secret` (write tmp →
`chmod 0600` → `rename` → fresh read sees new value).

### systemd-creds evaluation

`LoadCredentialEncrypted=` / `systemd-creds` (encrypted credentials) is a
**stronger optional future** mechanism. Gen-1 decision: **plain
`LoadCredential=` is LOCKED**; encrypted credentials are **SUPPORTED
FUTURE / DEFERRED**. Rationale: systemd 261 supports `LoadCredential` out of the
box without extra key management; encrypted credentials require a TPM2 or
provisioned host key and `systemd-creds` tooling not yet in the minimal
deployment target. No encrypted credential is required for Gen-1. Documented in
ADR-025 and `DEPLOYMENT.md`.

### Threat model

| Threat | Mitigation | Test |
|---|---|---|
| World-readable secret (`0644`) | Source `0600 root:root`, credential dir `0700` | `credential_directory_ownership_contract` |
| `synveil` reading admin source directly | `synveil` has no read on `/etc/synveil/credentials`; systemd (PID 1) exposes per-service copy `0400` | `runtime_account_isolation` (manifest + doc) |
| Secret in process environment (`DATABASE_URL`) | Production uses credential file, not env var; `Environment=` only carries non-secret path `%d/...` | `systemd_service_contains_credential_delivery_and_no_secret_env` |
| Secret printed in logs | Errors are generic, helper never logs URL; `no_secret_logging_in_errors` | `no_secret_logging_in_errors` |
| Secret overwritten on package upgrade | `CONFIG_DIRECTORY` + `CREDENTIAL_DIRECTORY` preserved, PACKAGE only | `credential_upgrade_preserves_byte_identical` |
| Secret deleted during ordinary uninstall | Ordinary uninstall preserves `credentials/*` | `credential_ordinary_uninstall_preserves` |
| Malicious symlink (`credentials` → external) | Install/uninstall use `realpath -m -s` + `rm -f` without follow | `credential_symlink_safety_does_not_follow_external` |
| Oversized credential file | Bounded 8 KiB, `metadata.len` + post-read check | `oversized_credential_fails` |
| Empty credential | Fails before DB | `empty_credential_file_fails`, `whitespace_only_credential_fails` |
| Ambiguous dual source (`file` + `DATABASE_URL`) | Fail closed `AmbiguousConfiguration` | `dual_source_is_ambiguous` |
| Credential rotation during timer | Next activation picks new file; one-shot is stateless, no cache | `credential_rotation_atomic_new_invocation_reads_new_secret` |

`/proc` / environment note: `EnvironmentFile` secrets are undesirable because
`proc` filesystem and debugging surfaces may expose environment to privileged
processes. Systemd credentials are placed in unswapped, read-only memory and
visible only to `synveil` (and root); root remains trusted administrator. We do
not claim root cannot see the secret.

### No secret cache

Loaded secret lives only for process lifetime in `DatabaseConfig` private
`database_url` field (redacted in `Debug`). Not persisted to PostgreSQL,
`/var/lib/synveil`, cache, worker table, or state file. Verified via
portable-core audit and `crates/metadata` cache checks.

## Linux systemd sandbox & runtime privilege hardening (Prompt 78)

Prompt 78 hardens the Gen-1 scheduled-maintenance service with the strongest
practical systemd sandbox proven compatible with the real one-shot runtime.
Decision: **evidence-validated sandbox, zero Linux capabilities, zero
writable system paths — LOCKED Gen-1** (ADR-026).

### Runtime requirement inventory (measured, not assumed)

| Requirement | Verdict | Evidence |
|---|---|---|
| Filesystem read | executable + libs, `/etc/synveil/*.env`, `$CREDENTIALS_DIRECTORY/database-url`, dynamic loader | gate runs succeed with `ProtectSystem=strict` and no `ReadWritePaths` |
| Filesystem write | **0 required** | no `ReadWritePaths`; idle/due/work gates exit 0; negative write probes denied |
| Network | `AF_UNIX` + `AF_INET` + `AF_INET6` | TCP gate runs + Unix-socket support retained; no other family allowed |
| Devices | **0 required** | `PrivateDevices=yes` + `DevicePolicy=closed`; `/dev/kmsg` probe denied; minimal private `/dev` only |
| `/proc` | minimal (`sqlx`/`tokio` self-introspection) | `ProtectProc=invisible` + `ProcSubset=pid` gates pass |
| Linux capabilities | **0 required/granted** | empty bounding + ambient sets; measured `CapEff=0`, `NoNewPrivs=1` |
| Kernel interfaces | **0 modification** | tunables/modules/logs/cgroups protections; gates pass |

### Accepted sandbox (production unit)

Privilege: `NoNewPrivileges=yes`, `RestrictSUIDSGID=yes`, empty
`CapabilityBoundingSet=` / `AmbientCapabilities=`. Filesystem:
`ProtectSystem=strict` (zero writable exceptions — `/var/lib/synveil` stays
read-only because the database-backed runtime never writes it),
`ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes` +
`DevicePolicy=closed`, `InaccessiblePaths=/etc/synveil/credentials`,
`UMask=0077`, `WorkingDirectory=/`, `RuntimeDirectory=synveil` (reserved,
unused by current runtime). Kernel: `ProtectKernelTunables=yes`,
`ProtectKernelModules=yes`, `ProtectKernelLogs=yes`,
`ProtectControlGroups=yes`. Process: `ProtectProc=invisible`,
`ProcSubset=pid`, `RestrictNamespaces=yes`, `RestrictRealtime=yes`,
`LockPersonality=yes`. Network:
`RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6` (IP allow/deny deferred —
deployments target different DB hosts; no localhost assumption is baked in).
Execution: `SystemCallArchitectures=native`, `MemoryDenyWriteExecute=yes`,
`SystemCallFilter=@system-service` minus `@mount` `@raw-io` `@reboot`
`@swap` `@module` `@debug` `@privileged` `@cpu-emulation` `@obsolete`
`@resources` with `SystemCallErrorNumber=EPERM`. (`@clock` covers only
time-setting syscalls; `clock_gettime`/`nanosleep` stay allowed.)

### Rejected / deferred directives (with reason)

- `PrivateUsers=` — **deferred**: persistent `synveil` ownership of
  `/var/lib/synveil` and `/etc/synveil` interacts poorly with user namespaces.
- `IPAddressDeny=any` + `IPAddressAllow=` — **deferred**: DB endpoint is a
  packaging choice, not a unit constant.
- `ProtectClock=` / `ProtectHostname=` — **deferred**: not required by the
  mandatory baseline; exposure is already 1.4 OK on systemd 261.2; no score-only churn.
- `RestrictFileSystems=` — **deferred**: brittle whitelist, no demonstrated
  benefit for this workload.
- `PrivateNetwork=yes` — **rejected**: would break PostgreSQL TCP.
- Any `ReadWritePaths=` under `/var` `/etc` `/usr` `/` — **rejected**: zero
  writes demonstrated; no hypothetical future permissions granted early.
- Any Linux capability — **rejected**: measured zero requirement.

### Credential compatibility

Hardening does not weaken Prompt 77: `LoadCredential=` delivery is read by
PID 1 before sandbox setup; the service reads only
`$CREDENTIALS_DIRECTORY/database-url` (read-only, per-service). The source
`/etc/synveil/credentials/database-url` (`root:root 0600`) is additionally
denied to the runtime via `InaccessiblePaths`. Operational note: systemd
requires an `InaccessiblePaths` target to exist — a missing credentials
skeleton fails closed at namespace setup (`226/NAMESPACE`); packaging always
creates it (`CREDENTIAL_DIRECTORY` in `MANIFEST`).

### Validation (disposable PostgreSQL 17, real systemd-managed execution)

- `systemd-analyze verify` service + timer: PASS.
- `systemd-analyze security` exposure: **4.5 → 1.4 OK on systemd 261.2** (informational;
  correctness authoritative).
- Hardened one-shot idle: exit 0, `tick Idle worker Idle`.
- Hardened due work: exit 0, one `MaterializedAndHandedOff` + one `Stepped`
  (`CREATED→SNAPSHOT_CAPTURED`), no sandbox denial.
- Hardened existing work: exit 0, `tick Idle` + one worker step.
- Negative probes under identical sandbox: writes to `/usr/bin`, `/etc`,
  `/run/user` denied; home-sentinel read denied; `/dev/kmsg` denied;
  `InaccessiblePaths` mechanism denied; private `/dev` minimal;
  `CapInh/Prm/Eff/Bnd/Amb` all zero, `NoNewPrivs=1`, unprivileged UID.
- Regressions: Prompt 75 identity, Prompt 76 install lifecycle (33 tests,
  installed unit byte-identical), Prompt 77 credential (15 tests), all live
  PG17 suites, Prompt 69 stress (`40P01=0`), 36/36 migrations from empty.
- Static tests: `crates/metadata/tests/linux_sandbox_hardening_units.rs`.

### Threat model (service-compromise blast radius)

Mitigated: binary/unit replacement, `/etc/synveil` modification, credential
source theft (DAC `0600` + `InaccessiblePaths`), home inspection, raw-device
access, kernel module/sysctl/cgroup manipulation, ptrace of unrelated
processes, dangerous namespaces, setuid escalation, privileged syscalls.
Remaining trust: root/systemd/package administrator (credentials visible to
PID 1); no claim against malicious host admin; no container-grade isolation
claim.

### No product change

Scheduler semantics, misfire policy, maintenance states, worker lease,
fencing, lock order, one-shot cadence, timer recurrence, credential
precedence, install lifecycle: all unchanged. Zero migrations, zero routes,
zero background runtime, zero `crates/core` contamination.

## Configuration contract

Configuration is typed, versioned and validated before a process becomes
ready. Precedence is documented and deterministic, for example:

```text
packaged defaults < configuration file < explicitly supported environment
references < command-line maintenance override
```

Secret values are referenced, not printed in effective-config output.

Required production categories include:

- canonical external URL and trusted Caddy/proxy networks;
- PostgreSQL endpoint/pool/timeouts and runtime role secret reference;
- storage backend type, identity, root/bucket/prefix, durability profile,
  capacity reserve and credential reference;
- session/cookie/CSRF and application master-key references;
- request/upload/part/quota/concurrency/deadline limits;
- journal/trash/version/staging/job/audit retention and internal metadata-purge
  reference-accounting state;
- the logical Trash retention override `SYNVEIL_TRASH_RETENTION_SECONDS` in
  whole seconds; when unset it uses the configurable 30-day default, and it
  controls logical retention eligibility and metadata purge only; it does not
  enable physical object purge or object-byte GC;
- metadata-only GC planning policy: `SYNVEIL_OBJECT_GC_GRACE_SECONDS`,
  `SYNVEIL_OBJECT_GC_LEASE_SECONDS`, and
  `SYNVEIL_OBJECT_GC_MAX_BATCH_SIZE`; defaults are 24 hours, 15 minutes, and
  100, with a hard batch maximum of 500; zero/invalid values fail startup
  configuration validation and do not enable physical deletion;
- internal GC-worker policy: `SYNVEIL_GC_WORKER_ENABLED` defaults to `false`;
  its `60`-second cycle, new-claim (`8`), active-operation (`2`),
  replica-action (`4`), execution (`2`), and replica-delete (`1`) caps are
  independently validated. `SYNVEIL_GC_WORKER_RETRY_BASE_SECONDS` and
  `SYNVEIL_GC_WORKER_RETRY_MAX_SECONDS` default to `30`/`900`,
  `SYNVEIL_GC_WORKER_MAX_ATTEMPTS` to `12`, and
  `SYNVEIL_GC_WORKER_SHUTDOWN_TIMEOUT_SECONDS` to `30`. The worker uses no
  database URL, storage root, or credential from this policy object; runtime
  composition owns those dependencies. See [STORAGE.md](STORAGE.md) for the
  complete variable list and bound relationships;
- log level/format/redaction, metrics and optional OTLP endpoint;
- bootstrap state and first-run exposure policy; the current HTTP contract has
  no setup-secret field;
- optional AI mode/provider/model/resource/privacy policy;
- optional Forgejo origins/credential references/webhook policy.

Production startup rejects unknown critical keys, invalid URLs, plaintext secret
values in prohibited fields, insecure cookies under a non-local URL, a schema
that is newer/unsupported, storage identity mismatch and internally
inconsistent limits. A config validation command runs without mutating data.

## Secret management

- Installation generates independent random database, session/token-verifier,
  CSRF, application-master and webhook/provider secrets as needed. A bootstrap
  setup secret is not part of the current HTTP contract and must not be
  invented in deployment configuration.
  Do not reuse one secret for multiple purposes.
- Prefer mounted files with restrictive host ownership/mode. Environment
  variables may leak through process inspection, crash/debug output or support
  tooling and are not the preferred long-lived production source.
- Compose `secrets` file mounts are not automatically encrypted at rest; their
  source files and backups remain operator responsibilities.
- No secret is baked into an image, checked into the repository, placed in a
  URL/command line, rendered to the web client or included in logs/diagnostics.
- Rotation is per secret type, supports an explicit short overlap where
  necessary, records audit, and has rollback/recovery instructions.
- The application master key is outside PostgreSQL. A system backup that omits
  it may restore metadata but lose access to encrypted integration/provider
  credentials. Backup and restore validation checks this explicitly.

## Container hardening

Production service definitions should:

- run with stable non-root UID/GID and minimal writable paths;
- set `no-new-privileges`, drop capabilities and avoid `privileged`;
- use a read-only root filesystem where runtime libraries support it;
- mount only the exact data/config/secret paths required, never host `/`, a
  home directory, workspace root or Docker socket;
- set bounded memory/CPU/PID/file-descriptor/temp-space policies appropriate to
  the host, while documenting how resource termination appears and recovers;
- provide an init/reaping strategy for subprocess-running parser/Git containers;
- pin promoted images by immutable version/digest and retain SBOM/checksum/
  provenance evidence;
- define health checks without embedding credentials on the command line.

The API and worker need object storage; Caddy and PostgreSQL do not. Optional AI
should not mount the entire canonical object root when a scoped capability/
stream can provide its one input. If direct backend access is selected, it uses
a separate read-only/scoped credential and cannot delete canonical objects.

## Caddy and HTTP behavior

Caddy supplies edge TLS and routing but does not own application security.
Production configuration must:

- redirect HTTP to HTTPS where applicable and automate or load a valid
  certificate for the canonical host;
- pass only normalized proxy headers and overwrite untrusted client-supplied
  forwarding values;
- preserve request/trace IDs under a documented policy;
- stream upload/download/range responses without buffering complete files;
- set timeouts compatible with resumable part sizes and slow-but-valid clients,
  while applying idle/deadline and connection limits;
- enforce a coarse maximum request/body/header policy, with authoritative
  per-user/session/part/quota validation repeated in the API;
- route `/api/v1/` only to the API and serve content-hashed web assets with
  appropriate cache headers; do not cache authenticated API responses by
  default;
- apply security headers and exact host/origin behavior; never expose internal
  storage paths or dependency errors.

HSTS is enabled only after hostname/certificate recovery is understood. A
development HTTP exception is loopback/isolated and cannot be copied into the
production profile silently.

## Installation and first-run UX

The eventual packaged flow is:

```text
obtain a pinned release/Compose bundle
    ↓
declare and validate persistent database plus pre-authorized object/backup paths
    ↓
generate protected secrets and non-secret configuration
    ↓
start PostgreSQL, run one-shot migrations, start API/worker/Caddy
    ↓
open canonical HTTPS URL and read the bounded bootstrap status
    ↓
create first administrator, consume/close bootstrap, then sign in explicitly
    ↓
select/confirm an ObjectStore from the host-authorized candidates
    ↓
run storage/health/system-backup readiness checklist
```

`git clone ... && docker compose up -d` may remain a contributor convenience.
Stable operators receive immutable release assets and exact version pins rather
than the moving default branch.

The install preflight verifies supported architecture, container runtime/
Compose version, port and hostname/TLS prerequisites, directory ownership,
storage identity/capabilities, available capacity/inodes, database connectivity
and config/secret permissions. It does not change an existing deployment until
the operator confirms the resolved paths.

The browser setup can make storage selection understandable without becoming
an arbitrary host-filesystem browser. Compose/the operator first mounts and
allowlists candidate roots or configures named backends; after authentication,
bootstrap selects one candidate, verifies/creates its Synveil storage identity
under an explicit confirmation, and records the non-secret backend reference.
Backend credentials remain mounted secrets and never round-trip to browser
JavaScript. A later backend migration uses the copy/verify/switch procedure,
not a new path string.

First-run bootstrap in the current HTTP/web phase:

1. is available only while no administrator exists;
2. accepts the canonical `login`, `login_key`, and `password` fields in a
   strict bounded JSON body; no setup-secret field is accepted;
3. checks browser provenance when `Origin` or `Sec-Fetch-Site` is present, so
   the deployment must keep the route on a trusted/private or correctly
   terminated TLS network until a later installer/secret-gate contract exists;
4. serializes concurrent claims and commits administrator plus bootstrap-close
   atomically through the existing service;
5. returns safe status metadata, does not issue a session, and closes
   permanently until a separately reviewed maintenance action exists. The web
   client then performs explicit login.

This phase has no distributed rate-limiter subsystem. The body bound, generic
errors, no-store responses, provenance checks, and deployment network boundary
are the current controls; operators must not expose an open setup endpoint to
an untrusted public network.

No first-run step requires manual SQL or editing object metadata.

## Schema migration lifecycle

- A distinct one-shot migrator holds a PostgreSQL advisory lock, validates
  released migration checksums and applies ordered forward migrations.
- API/worker do not race to migrate. They remain unready while a supported
  migration is running and refuse an unknown newer schema.
- Transactional DDL is used where supported. Non-transactional operations have
  a resumable state/check and documented interruption behavior.
- Use expand/backfill/contract changes. Large backfills run as restartable,
  rate-bounded jobs with progress metrics, not one unbounded startup lock.
- Released migration files are immutable. Correction is a new migration and an
  upgrade fixture.
- If mixed application versions can run, readers for old/new forms deploy
  before new writers. Initial single-host maintenance upgrades may instead
  deliberately stop all old writers and document downtime.
- Schema/data changes never assume a wipe. A binary downgrade is allowed only
  by the explicit compatibility matrix; otherwise rollback means complete
  pre-upgrade restore.

## Health and readiness

### Endpoint semantics

| Signal | Meaning | Dependency behavior |
|---|---|---|
| `/health/live` | Process runtime/event loop can answer | No expensive dependency probes; failure means restart may be useful. |
| `/health/ready` | Process can safely receive its required traffic | Valid config/schema; bounded PostgreSQL check; selected object backend identity and recent cached probe; required secret availability. |
| Restricted health details | Operator view of DB, object capacity/integrity, workers, jobs, backups, AI/integrations and migrations | Authenticated/admin or internal only; returns safe states, timestamps and stable errors, not credentials/paths. |
| Worker heartbeat | Worker is claiming/completing required job classes | Stored/aggregated with freshness; stale worker degrades jobs but does not make core reads false-live. |

Liveness does not query every dependency. Readiness does not scan all objects or
create a test object on each probe. Periodic storage health jobs perform bounded
write/read/delete probes in a reserved health namespace, cache the result and
feed readiness according to policy.

Core API readiness does not require AI, Forgejo, thumbnails, OCR or semantic
search. Their status is `DISABLED`, `READY`, `DEGRADED`, `STALE` or `FAILED` in
the restricted system view. PostgreSQL or canonical object-backend failure may
make mutations unready while carefully defined metadata reads continue; the
edge should not restart-loop a healthy process because a dependency is down.

### Human-readable health model

Personal / Home Mode presents product states such as:

```text
Storage       Healthy
Database      Healthy
Backups       Healthy
Remote access Connected
Devices       4 connected
```

The user-facing layer translates stable error codes and diagnostics into an
action, for example: “Your Synveil storage is almost full. 182 GB remains.
[Manage storage].” Administrator diagnostics retain `ENOSPC`, migration state,
backend identity, job age, and a correlation ID behind an authenticated view.
Developer diagnostics retain redacted logs/traces. These layers must not
disagree about the underlying health state.

### Automated maintenance

The native/personal service may schedule database maintenance, GC, integrity
scans, backup verification, thumbnail/cache cleanup, certificate renewal,
service recovery, log rotation, storage health checks, retention cleanup, and
update readiness. Each operation is safe, observable, recoverable, and bounded.
Destructive maintenance uses `inspect → plan → validate → execute → verify`,
supports dry-run/report where practical, and never silently deletes the only
recoverable copy.

## Jobs and worker operations

PostgreSQL is the initial durable queue/outbox. Workers claim with short
transactions and lease generations, execute outside the claim transaction,
then conditionally complete. Operations expose:

- queue depth and oldest eligible age by bounded job class;
- attempts, lease expiry/steal, running duration, success/failure and dead
letters;
- last successful staging/orphan/integrity/retention/GC/storage-health job;
- per-class concurrency, priority and backpressure;
- safe administrative pause/resume/retry/dead-letter inspection with audit.

The implemented object-GC execution boundary continues the planner after a
`READY` lease. It creates durable operation/action rows before every external
effect, moves the Object to `GC_DELETING`, and uses short transactions in this
order: candidate -> canonical Object -> operation/action. It renews and
revalidates the matching lease/generation plus zero FileVersion references and
active holds before each replica action; ObjectStore I/O occurs outside those
transactions. Exact replicas are ordered deterministically, deleted one at a
time using conditional evidence where available, and reconciled after any
ambiguous response. Candidate/ObjectReplica/Object removal occurs only after
all replicas are proven absent.

The implemented `synveil-worker` is an opt-in private runtime, not an API
process extension or public control surface. Its runtime loop calls the
transport-neutral `run_once()` coordinator, sleeps at the configured interval,
and listens for shutdown. A cycle reports bounded metadata reconciliation,
then reclaims due incomplete operations before considering new candidates; any
recovery claim suppresses new destructive work for that cycle. It applies at
most one replica action per selected operation, caps operation task and
storage-delete concurrency independently, and releases a nonterminal planning
lease after each slice so that the next due cycle has to reclaim current
generation fencing.

Replica retry attempt count and next due time are persisted in PostgreSQL. The
server clock schedules a bounded exponential retry delay with at most 10%
deterministic jitter. Transient database/storage, stale lease, and ambiguous
outcomes do not report completion; persisted identity/evidence/configuration
faults or an exhausted retry budget become `NEEDS_ATTENTION`. On Ctrl-C the
binary stops claiming, drains only the configured bounded current cycle, and
leaves a timed-out fenced action for normal Prompt 28 reconciliation after a
restart. General API health/readiness remains independent of worker backlog or
an individual GC failure. Safe per-cycle counts, status, duration, and redacted
error classes are logged; an operator does not receive storage keys, paths,
credentials, or a delete command.

Reconciliation is bounded and metadata-only: it reports inconsistent
candidate/operation/action/Object lifecycle states but does not recursively
inventory a storage root. Unknown physical files are not auto-deleted in this
phase. Backup/share/sync hold producers remain unimplemented.

Worker termination expires a bounded lease and repeats idempotently. Poison
jobs become terminal rather than hot-looping. Optional AI/photo/Git work uses
separate concurrency classes and cannot starve reconciliation, integrity,
retention or backup jobs.

No early deployment needs Kafka, RabbitMQ, NATS or Redis. A broker may later
fan out from the PostgreSQL outbox only after a measured ADR demonstrates
equivalent retry/idempotency and operations behavior.

## Observability

### Structured logs and traces

Rust uses `tracing`. Production JSON records include service/version,
timestamp, severity, request/trace/operation ID, route template, stable error
code, outcome, duration and bounded byte counts. Principal/device identifiers
are redacted/pseudonymous. Worker spans connect outbox/job/source version,
object/backend and retry without recording content or storage keys.

Never log passwords, cookies, bearer/share/recovery tokens, secret/config
values, file contents, full sensitive paths/names, AI payloads, SQL parameters,
authorization headers or credential-bearing URLs. A debug mode does not disable
redaction.

OpenTelemetry export is operator-configured and off by default. A local log is
not automatically sent to Synveil or another hosted service.

### Metrics

Expose a Prometheus-compatible internal endpoint or equivalent bounded exporter
without requiring a bundled Prometheus/Grafana stack. Metrics include:

- HTTP requests/in-flight/duration/errors and streamed bytes by route template,
  method/status class and stable error—not user/file/token;
- active upload sessions/parts, staged bytes, completion/checksum/disk/quota
  failures and stream memory/concurrency;
- DB pool wait/use, transaction retry/deadlock, migration and query-family
  duration;
- object read/write/range latency/errors, capacity/inodes/reserve, missing/
  corrupt/quarantined replicas and reconciliation/GC findings;
- change-feed head/retention/cursor-expiry and device lag/backlog in bounded
  buckets;
- job depth/oldest age/attempt/dead-letter/worker heartbeat by bounded class;
- latest successful backup/snapshot/restore and restore verification failures;
- optional photo/AI/Git queue freshness, provider/connector status and failure
  class without payload/repository names.

High-cardinality labels are prohibited. Audit records, not metrics, answer
user-specific security questions.

### Alerting baseline

Operators need actionable warnings for database/object unavailability, storage
reserve/inode exhaustion, unexpected storage identity, checksum corruption,
orphan/reference drift, stalled required jobs, failed/late system or user
backup, repeated authentication/share abuse, migration failure, certificate
expiry, secret decryption failure and optional provider/connector degradation.

Thresholds are configuration/load-profile decisions measured during phases;
documentation does not invent universal marketing SLOs. Every alert links to a
runbook, distinguishes symptom from destructive action, and avoids automatic
purge/repair on uncertain evidence.

## Capacity and performance operations

Capacity planning distinguishes:

- canonical logical bytes, retained historical/backup bytes and physical bytes;
- committed objects, active staging, orphan grace, derivatives, indexes,
  database/WAL and system-backup space;
- byte capacity and inode/object/row counts;
- CPU/memory/blocking threads/DB connections/network and external-provider
  quotas.

Maintain configurable disk/inode reserves. New staging/content commits stop
before critical exhaustion while reads, purge/recovery and operator diagnostics
retain headroom. An emergency reserve is not counted as user quota.

Benchmark evidence records hardware, filesystem/backend, TLS/proxy, software/
config version, file-size/directory distribution, concurrency, warm/cold cache,
duration, error rate, CPU/memory/disk/network and tail latency. Relative
regression budgets precede product claims. Performance tuning may not weaken
fsync, checksums, authorization, idempotency or restore.

## Synveil system backup

The product's user backup feature does not automatically protect the Synveil
server itself. Operators require a coordinated **system backup** of:

1. PostgreSQL metadata, journals, jobs, audit and idempotency state;
2. every referenced canonical object namespace/backend or an independently
   durable backend snapshot with a precise restore point;
3. effective non-secret configuration, release version, migration checksums and
   storage identity;
4. application master key and other irreplaceable secrets through a separate
   encrypted recovery channel;
5. optional required connector/model license metadata and external-storage
   configuration.

Database-only backup cannot restore file bytes. Object-only backup cannot
reconstruct names, ownership, versions, manifests or authorization. A copy on
the same disk is not protection against disk loss.

### Initial coordinated backup procedure

The safest first supported online model is a documented maintenance window:

1. validate target, capacity, encryption and prior backup status;
2. enter maintenance/drain mode; stop new mutations/uploads and pause workers;
3. wait a bounded time for active database transactions/object finalizations,
   then stop API/worker writers if necessary;
4. record product/schema/config/storage identity and a database consistency
   point; create a reviewed `pg_dump` or supported PostgreSQL snapshot;
5. copy/snapshot the immutable committed object namespaces and required
   backend metadata without concurrent writers;
6. securely capture configuration and irreplaceable key references/material;
7. create a manifest with component versions, counts/sizes/checksums and
   staging policy; encrypt and transfer to a separate failure domain;
8. restart services, run bounded health/invariant checks and record success;
9. periodically restore the set into an isolated clean environment.

Incomplete upload staging may be included to preserve resumability, or excluded
under a documented policy that marks restored open sessions expired/retryable.
The choice is recorded in the backup manifest and never affects committed
objects.

Future online snapshots need an explicit mutation barrier/high-water mark and
backend snapshot guarantees before they replace the maintenance method.

### Backup retention and safety

- Define operator-selected RPO/RTO and retention; do not advertise a universal
  guarantee.
- Keep at least one encrypted off-host/offline or independently administered
  copy where practical. A 3-2-1-style policy is recommended guidance, not a
  Synveil guarantee.
- Never prune the last known-good set immediately after creating an unverified
  new set. Verify manifest, restore metadata, sample/full object reads according
  to policy, and then apply retention.
- Restrict backup credentials from normal API/worker roles. Backup logs do not
  contain master keys, file names or database secret values.

## Disaster restore

A restore uses a clean target and never points an unvalidated process at the
only backup copy.

1. Provision the documented compatible Synveil/PostgreSQL/runtime version and
   empty validated persistent paths.
2. Verify backup manifest, encryption/key availability, release/schema and
   object backend identity before writing.
3. Restore object data to a new namespace/path and verify counts/checksums/
   samples according to the manifest.
4. Restore PostgreSQL with no API/worker writers, apply only the documented
   compatible migration sequence and restore configuration/secret references.
5. Run read-only invariant checks: object references/locations, library roots,
   journal epochs/heads, committed snapshots, jobs/leases and quarantined data.
6. Start API in maintenance/read-only mode, verify authenticated representative
   file/version/range reads and a backup restore to a non-destructive
   destination.
7. Expire/reconcile leases, staging and jobs according to the backup manifest;
   never bulk-delete uncertain objects.
8. Rotate credentials that may have been exposed by the incident, enable
   writers, monitor and record the recovery evidence.

Missing canonical objects remain explicit `MISSING`/corrupt incidents; metadata
is not silently deleted to make the check green. If the master key is missing,
report which encrypted secrets/data cannot be recovered rather than generating
a replacement and pretending continuity.

## Release and upgrade runbook

### Release artifacts

Promoted releases provide immutable image versions/digests, source/checksums,
SBOM/license inventory, Compose/Caddy/config schema, migrations, release and
security notes, known limitations, supported source-version matrix, resource
change, backup/rollback instructions and test provenance. Operators do not pin
to `latest`.

### Upgrade procedure

1. Read release notes and confirm the current version is a supported source.
   Perform required stepping upgrades rather than skipping silently.
2. Validate configuration with the new schema; check storage identity,
   database/object health, capacity/inodes and no unresolved corruption.
3. Create and verify a complete pre-upgrade system backup in another failure
   domain. Record its manifest and restore method.
4. Pull/verify exact release artifacts and retain prior artifacts/config.
5. Enter maintenance; drain uploads/mutations; pause/stop workers and old API
   writers as the compatibility plan requires.
6. Run exactly one migrator under advisory lock. If a resumable backfill is
   required, follow its version-specific sequencing and metrics.
7. Start database-dependent services in order, wait for bounded readiness, then
   Caddy. Optional AI/Git failures do not block core readiness.
8. Run post-upgrade smoke/invariant checks: login, metadata list, representative
   file hash/range, idempotency record, journal head/change page, backup snapshot
   list and non-destructive restore; inspect job/worker health.
9. Exit maintenance and monitor error, storage, DB, job and backup signals for
   the documented soak period.

### Failure and rollback

- If migration did not commit and prior schema remains compatible, correct the
  cause and retry the same immutable migration under its documented rule.
- If the new binary fails but schema/format remains backward compatible, a
  documented binary/config rollback may be used.
- If a forward migration or new writer made incompatible changes, stop writers
  and restore the complete pre-upgrade database/object/config/secret set. Do
  not start an old binary against a newer unknown schema.
- Object-format migration uses copy, verify, transactional switch, rollback
  window, then retirement. Never rewrite the only canonical representation in
  place.
- A failed upgrade never recommends wiping PostgreSQL or reinitializing the
  object root.

## Automatic update architecture

Native Personal / Home installations may eventually offer stable and beta
channels. An update coordinator must verify signed release artifacts, check
platform/runtime compatibility, validate capacity/configuration/health, drain
services, run exactly the allowed migration path, and require a verified backup
before dangerous schema or storage-format changes. It records progress and
failure, retains prior artifacts/configuration when rollback is possible, and
documents when rollback requires restoring the complete pre-upgrade recovery
set. It never uses unsigned packages or silently replaces the database/object
root.

Notification-only or explicit opt-in updates are valid early profiles.
Administrator-controlled pinned upgrades remain valid for Compose and Advanced /
Server Mode. Automatic security-only updates are not assumed safe until the
update, air-gapped, backup, migration, and support policies are accepted.

## Uninstall semantics

The deployment manager must distinguish these resources:

```text
application binaries
configuration
PostgreSQL/database state
stored user objects
cache and temporary data
logs and audit evidence
credentials and key material
system/independent backups
```

“Remove Synveil application” stops services and removes binaries while keeping
data/configuration according to the selected retention choice. “Permanently
delete Synveil data” is a separate destructive workflow with an explicit scope
summary, confirmation, and recovery warning. Uninstall must not casually delete
PostgreSQL or the object root, and a reinstall must be able to discover a kept
storage identity rather than initialize over it.

## Machine migration and recovery UX

A guided migration from an old computer to a new computer eventually follows:

```text
prepare migration
    → inspect source and destination
    → validate database/object/configuration/key/capacity/version
    → copy or transfer
    → verify references, checksums, and health
    → activate destination
    → reconnect or re-register devices
```

The operation is durable, resumable, and uses the same
`inspect → plan → validate → execute → verify` rule as other irreversible
operations. It must address PostgreSQL metadata, object replicas, application
master keys, device identity/credential rotation, hostname/TLS, remote-access
configuration, old-instance coexistence, and rollback. A missing master key or
incomplete object transfer is a visible recovery blocker, never a reason to
generate a replacement and claim continuity.

The user-facing recovery surface should offer understandable workflows for an
accidental deletion, previous version, lost laptop, failed drive, corrupted
object, broken update, database recovery, and moving to a new server. Low-level
operator instructions remain available as an escalation path.

## Failure-operation matrix

| Failure | Service/health behavior | Operator action |
|---|---|---|
| PostgreSQL unavailable | Mutations rejected; API not ready for dependent work; no successful untracked object commit | Restore DB connectivity; do not purge staged/object bytes; inspect transactions/jobs after recovery |
| Object backend unavailable | New content commits and byte reads fail with retryable stable error; safe metadata may remain | Restore backend/credentials/identity; verify representative objects and reconciliation before writes |
| Disk/inodes near reserve | Stop new staging before exhaustion; preserve read/recovery headroom; alert | Add/migrate capacity or explicitly reduce retained data through policy; never broad-delete paths |
| Object durable, DB commit failed | Unreferenced orphan protected by lease/grace; request fails | Let reconciliation verify/reuse/delete after grace; inspect drift metric, not manual path deletion |
| DB committed, response lost | Retry returns persisted outcome | Client retries same idempotency key; operator action normally unnecessary |
| API crashed | Caddy returns unavailable; worker may continue safe jobs | Restart same version; verify readiness and active upload/session recovery |
| Worker crashed/stale | Core commits continue after durable outbox; job age grows | Restart; leases expire and handlers repeat idempotently; inspect poison jobs |
| AI failed | AI/OCR/search-derived status degraded/stale; core ready | Disable/restart/retry optional profile; never roll back canonical data |
| Forgejo unavailable | Connector stale/errored; core ready and verified backups readable | Correct endpoint/credential; bounded reconciliation; no core restart needed |
| Checksum mismatch | Bad `ObjectReplica` becomes `CORRUPT`/`MISSING`; a healthy verified replica keeps the `Object` readable; otherwise the `Object` is `QUARANTINED` and affected reads fail loudly | Preserve evidence, locate a verified replica/system backup, restore and audit blast radius; high-severity alert |
| Storage identity changed/missing | API/worker fail readiness rather than treating objects as deleted | Correct mount/config; validate exact resolved path/marker; never initialize over uncertain data |
| Migration interrupted | Services remain in maintenance/unready according to migration state | Follow immutable migration resume/restore procedure; do not edit migration history |
| TLS/certificate failure | Caddy cannot serve trusted production HTTPS | Repair DNS/ACME/certificate from operator runbook; do not expose insecure public HTTP fallback |
| Master key missing | Integration/provider secret decryption fails; affected optional features disabled | Recover key from secure backup or reconfigure/rotate credentials; never silently replace and claim recovery |

## Scale evolution gates

### S3/MinIO

Promote only after the adapter passes the common streaming, range, conditional
create, multipart, checksum, read-after-write, retry, corruption, orphan, delete
and storage-migration suite. S3 ETag is not the plaintext hash. Bucket policy
limits Synveil to its prefix and deletes remain server-controlled.

### Worker replicas

Add when queue age or CPU work exceeds the declared envelope. First prove lease
generation, duplicate execution, long-job renewal, class fairness, dead-letter,
shutdown/drain and database-pool behavior with multiple workers.

### API replicas

Add after shared object read-after-write, session/revocation lag, rate limiting,
idempotency, per-library clock locking, proxy routing and deployment/migration
coordination tests. No local-only staging/session state may be required by a
specific replica.

### Broker, read replicas and Kubernetes

- A broker is justified only by measured PostgreSQL job/outbox pressure or a
  required delivery topology. PostgreSQL remains atomic handoff until an ADR
  proves an equivalent no-loss boundary.
- Read replicas serve only explicitly stale-tolerant queries, never immediate
  authorization, idempotency outcome, upload completion or cursor allocation.
- Kubernetes is an alternative deployment after Compose operations mature, not
  a correctness feature or a production prerequisite.

Every scale change has a security boundary, cost/operations owner, mixed-version
plan, backup/restore and rollback proof.

## Deployment validation matrix

Before a profile is called supported, CI or a release lab validates:

- clean install and one-time bootstrap;
- restart after host/container/process termination;
- upgrade from every supported source version and documented rollback/restore;
- PostgreSQL/object/worker/AI/Forgejo outage behavior;
- disk/inode reserve, quota, checksum corruption and unexpected mount identity;
- TLS, headers, proxy trust, private-port scan and secret/log redaction;
- system backup plus clean isolated restore;
- declared architecture/OS/filesystem/backend and container runtime versions;
- representative streaming/range/upload/sync/backup workloads under the
  published capacity envelope.

For every claimed first-class host profile, the lab also validates native or
guided install, service startup/shutdown/crash recovery, storage picker safety,
managed database lifecycle where applicable, user-facing health translation,
signed update/uninstall-with-data-preservation, and clean machine migration.
Windows, macOS, Linux Desktop, and Linux Server are not represented by a Linux
Compose test alone.

## Deployment `OPEN DECISION` items

### OPEN DECISION OD-D01: packaged persistent-path defaults

- **Owner:** DevOps, Storage, Project owner
- **Needed by:** first production Compose bundle
- **Options:** named volumes; validated bind mounts under an operator path;
  installer-managed directory
- **Recommendation:** support named volumes for simple install and explicit
  validated bind mounts for NAS/capacity control; never infer a broad host path.
- **Decision evidence:** backup/restore UX, rootless ownership, NAS capability
  tests and cross-platform Compose behavior.

### OPEN DECISION OD-D02: initial PostgreSQL system-backup method

- **Owner:** Database, Operations
- **Needed by:** `SV-G1-STORAGE` production readiness
- **Options:** maintenance-window `pg_dump`; physical snapshot/base backup;
  operator external PostgreSQL backup integration
- **Recommendation:** maintenance-window logical backup for the initial
  single-host profile, with physical/external adapters added only under a tested
  compatibility/runbook contract.
- **Decision evidence:** database size/load measurements, point-in-time needs,
  restore time and exact version support.

### OPEN DECISION OD-D03: AI data-plane privilege

- **Owner:** AI, Security, Architecture
- **Needed by:** `SV-G10-AI-OPTIONAL`
- **Options:** Rust worker streams scoped input/output; short-lived internal
  capability URL; restricted direct DB/object credentials
- **Recommendation:** scoped Rust/API-mediated capability with no broad object
  delete or metadata authority; direct access only if measured transfer cost
  justifies it and least-privilege conformance proves safety.
- **Decision evidence:** local/S3 compatibility, large-input throughput,
  revocation, audit and compromise blast-radius tests.

### OPEN DECISION OD-D04: supported host/runtime matrix

- **Owner:** Release, QA, Project owner
- **Needed by:** first alpha
- **Options:** one container/runtime baseline; selected first-class host profiles;
  broader architecture/runtime matrix
- **Recommendation:** publish the first tested matrix across Windows, macOS,
  Linux Desktop, and Linux Server plus the declared Advanced / Server runtimes;
  label untested environments community/experimental until their
  install/upgrade/restore suite passes.
- **Decision evidence:** CI/release-lab capacity, filesystem/network behavior,
  native/guided install, service lifecycle, update, uninstall, and migration
  evidence for every claimed host.

### OPEN DECISION OD-D05: automatic update behavior

- **Owner:** Product, Release, Security
- **Needed by:** first beta
- **Options:** notification only; opt-in download; opt-in automatic maintenance
- **Recommendation:** notification/manual pinned upgrade first. Never mutate a
  self-hosted installation automatically without explicit opt-in, verified
  backup and a rollback contract.
- **Decision evidence:** security patch urgency, operator UX, backup/upgrade
  reliability and air-gapped use.

### OPEN DECISION OD-D06: native service and installer privilege model

- **Owner:** Platform / Distribution, Release, Security
- **Needed by:** native installer architecture gate
- **Options:** one privileged supervisor; per-user service plus narrowly scoped
  helper; OS-native service per platform; user-session-only service
- **Recommendation:** one platform-neutral lifecycle port with the least-
  privilege OS-native adapter that can protect the data root and recover core
  services.
- **Decision evidence:** elevation/IPC threat tests, reboot/sleep/crash
  recovery, multi-user host behavior, update/uninstall, and support matrix.

### OPEN DECISION OD-D07: managed PostgreSQL packaging

- **Owner:** Database, Release, Security, Product
- **Needed by:** Personal / Home Mode implementation
- **Options:** bundled/private PostgreSQL; system-managed service; packaged
  dependency; external PostgreSQL only
- **Recommendation:** Synveil-managed private/system service for supported
  Personal / Home platforms, external PostgreSQL for Advanced / Server Mode;
  preserve one canonical PostgreSQL model.
- **Decision evidence:** package/security patch cadence, data-directory
  ownership, Windows/macOS/Linux backup/restore, upgrade, uninstall, and
  resource/support cost.

### OPEN DECISION OD-D08: machine migration package

- **Owner:** Backup / Recovery, Database, Storage, Clients, Release
- **Needed by:** supported migration gate
- **Options:** guided local transfer; portable encrypted migration archive;
  coordinated backup/restore; administrator-only procedure
- **Recommendation:** guided workflow over verified backup/restore primitives;
  add an archive only after key, identity, format, and resume semantics are
  fully specified.
- **Decision evidence:** clean destination restore, interrupted transfer,
  device re-registration, hostname/remote-access change, and rollback tests.

## Production desktop launch and user supervision (Prompt 99)

Prompt 99 makes the existing Qt desktop usable without manually starting the
background process while preserving the two-process lifecycle boundary:

```text
synveil-desktop (Qt/QML/tray)
    -> BackgroundClientManager + DesktopController
       -> Prompt 96 local IPC
          -> synveil-client (DesktopSyncHost/SyncRuntime/writer lock)
```

The bridge calls the typed `BackgroundClientManager` after controller startup.
The manager performs one bounded endpoint/supervisor inspection and may request
one client start for endpoint absence, a stopped client, or an inactive user
supervisor. A profile-scoped gate coalesces concurrent requests and a bounded
cooldown prevents reconnect-driven spawn storms. Endpoint-security,
protocol-incompatible, malformed-control, writer-conflict, and terminal states
are fail-closed and never cause replacement launch attempts. Prompt 95's writer
lock remains the final same-profile ownership boundary.

The manager never owns sync correctness, library/root observation, credentials,
checkpoints, SQLite, `DesktopSyncHost`, `SyncRuntime`, or raw Prompt 96 frames.
QML sees only safe categories and a generic launch label. GUI close stops only
controller-owned work; it does not send Prompt 96 `Shutdown` or stop the
client. A configured user supervisor can therefore recover the client while
the GUI is closed, and the controller can reconnect to a fresh generation when
the GUI is open.

### Linux user service

The production client unit is packaged at
`/usr/lib/systemd/user/synveil-client.service`, not the system unit path. It
uses the existing source-defined process contract:

```ini
[Service]
Type=simple
ExecStart=/usr/bin/synveil-client
Restart=on-failure
RestartSec=30s
RestartPreventExitStatus=78
```

`StartLimitIntervalSec=5min` and `StartLimitBurst=5` bound repeated crashes.
Exit 78 is the current permanent configuration-error classification; exit 70
is the generic bootstrap/runtime failure. The unit has no
`network-online.target` dependency because Prompt 92 owns network recovery.

Autostart is explicit and reversible:

```sh
systemctl --user daemon-reload
systemctl --user enable synveil-client.service
systemctl --user start synveil-client.service
systemctl --user status synveil-client.service
systemctl --user disable synveil-client.service
systemctl --user stop synveil-client.service
```

The package-neutral installer, DEB/RPM hooks, and GUI do not silently enable
or start the unit. An explicit disable is not undone by a later GUI launch.
Disposable tests use linked temporary units and clean the user manager after
the lifecycle check.

### Windows user task

The Windows adapter uses a current-user Task Scheduler definition with a logon
trigger, `InteractiveToken`, `LeastPrivilege`, exact canonical sibling
`synveil-client.exe`, `IgnoreNew`, and finite restart interval/count. It calls
fixed `System32\\schtasks.exe` with explicit argv. It does not use a Windows
Service, `SYSTEM`, elevation, stored passwords, registry `Run`, or shell
interpolation. Task identity contains only a sanitized profile identifier and
no token, credential, root path, or server URL.

### Package contents and validation

The package-neutral `deploy/install/MANIFEST` is authoritative for DEB and RPM.
The Linux package contains `synveil-client`, `synveil-desktop`, the user unit,
ordinary desktop entry/icon, existing maintenance files, and `LICENSE`/`NOTICE`.
The desktop entry is not a login-autostart hook. Actual DEB/RPM artifacts are
audited for exact payload paths, modes, byte parity, no secrets, and no source,
`/tmp`, build, or development paths.

The Windows path creates an unsigned reproducible ZIP rather than an installer.
It contains both EXEs, `qt.conf`, target Qt DLLs/QML modules/plugins,
`platforms/qwindows.dll`, required C++ runtime files, and notices. Native
Windows uses `windeployqt`; Linux cross packaging requires a genuine Windows Qt
prefix and an explicit closure allowlist. PE import auditing rejects missing
non-system dependencies and Linux/development leakage. Native Windows task
registration, package startup, and tray interaction are runtime gates and are
not claimed by Linux static or cross-build evidence.

See [`docs/en/DESKTOP_LAUNCH.md`](DESKTOP_LAUNCH.md),
[`docs/vi/DESKTOP_LAUNCH.md`](../vi/DESKTOP_LAUNCH.md), and
[`ADR-041`](adr/ADR-041-production-desktop-launch-orchestration.md).
