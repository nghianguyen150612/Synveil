# Managed server service installation

Status: **Implemented foundation — Prompt033.** This contract qualifies the
initial Ubuntu 24.04 and Fedora 42 x86_64 machine-service topology. It establishes
`INFRASTRUCTURE_READY`; it is not whole-product readiness.

## Ownership and topology

The desktop package remains independent. A closed `SERVER_PACKAGE` installs
verified siblings beneath `/opt/synveil/server/<runtime-id>/` and atomically
selects `/opt/synveil/server/current`. It contains `synveil-api`,
`synveil-worker`, and `synveil-server-migrate`, but no desktop/client, secret,
database cluster, configuration, or object data. PostgreSQL runtime is another
replaceable artifact under `/opt/synveil/postgresql/17/<runtime-id>/`; its exact
version, provenance, license/notices, inventory, SHA-256 and compatibility are
verified before a root-owned `current` pointer changes.

~~~text
synveil-server.target (enabled at multi-user.target)
  synveil-postgresql.service        [managed profile only]
    -> synveil-server-migrate.service (oneshot)
       -> synveil-api.service
          -> synveil-worker.service
~~~

API, migration, and worker run as persistent non-login `synveil:synveil`.
PostgreSQL runs as separate `synveil-postgres:synveil-postgres`, with no
supplementary privileged groups. Neither runs as root or the desktop user.
The target starts at boot independently of login. Stop ordering reverses the
dependency graph: worker, API, then managed PostgreSQL.

## PostgreSQL 17 and cluster identity

Managed PostgreSQL listens only on `127.0.0.1` in the bounded 55432–55463
range and uses its private `/run/synveil/postgresql` socket. SCRAM is mandatory
for application TCP. The persisted endpoint is authoritative and an occupied
endpoint is a repair condition, never silently reselected. AdvancedExternal
must prove PostgreSQL major 17 and credentials, but Synveil never installs,
starts, stops, updates, or removes that operator-owned database.

Durable state is `/var/lib/synveil/postgresql/17/data` mode 0700 owned by the
PostgreSQL identity. The sibling `.synveil-managed-postgresql.json` binds a
UUIDv7 managed-cluster ID to the server installation, runtime artifact, data
path, major, and the numeric system identifier obtained from PostgreSQL 17.
Directory presence or `PG_VERSION` alone never proves ownership. Missing,
foreign, wrong-major, partial-init, marker-mismatch, identifier-mismatch, and
permission states are distinct. `initdb` is permitted only for a proved owned
empty prepared target, runs as `synveil-postgres`, and is never blindly retried.

Provisioning reconciles the dedicated `synveil` database and non-superuser
`synveil` role without drop/recreate. It consumes the existing protected
`database-password` without command-line exposure and materializes the
password-bearing `database-url` only through the P031 secret writer. PostgreSQL
does not receive that password during normal daemon operation.

## Plan, interruption, storage, and credentials

A UI-neutral plan binds server ID, configuration generation/fingerprint,
storage ID/root identity, profile and exact runtime identities. Configuration
or storage changes make it stale. Preparing IDs are durably recorded before
mutation; resume inspects each effect before reconciling. OutcomeUnknown never
authorizes another ID/password, cluster, initdb, or storage initialization.

P032 `ConfiguredLocal` identity must validate before installation. For fresh
closed layout only the exact ObjectStore writable paths are handed to
`synveil`; unknown files or ownership require repair, and recursive chown is
forbidden. A root-owned generated unit drop-in uses reviewed systemd escaping
for exactly that verified root via `ReadWritePaths=`. PostgreSQL has no access.

PID 1 copies root-owned 0600 sources using `LoadCredential=`. Units expose only
`%d/database-url` and `%d/rebaseline-token-key` paths. They explicitly hide
`/etc/synveil/credentials`; no secret value appears in `Environment=`, an
environment file, configuration, arguments, or logs.

## Migration, health, shutdown, and sandbox

The one-shot migration binary loads managed configuration and credential,
requires PostgreSQL 17, and invokes the sole `MigrationRunner`. API and worker
cannot start before it succeeds. API remains `127.0.0.1:3000` until P034 and
handles SIGTERM/SIGINT through Axum graceful shutdown. Worker has no listener,
announces entry only after configuration, database, migration and existing-only
storage open, then applies bounded drain on SIGTERM/SIGINT.

Readiness is bounded: PostgreSQL runtime/major/cluster/endpoint/database/role
and authenticated credential; HTTP `/health/live` then `/health/ready` without
redirects; and active worker main loop. PID or open port alone is insufficient.
The units use journald, finite restart limits, no capabilities, no privilege
escalation, strict filesystem/home/device/kernel controls and narrowly allowed
local networking. PostgreSQL receives its separately reviewed writable paths.

## Repair and data-preserving removal

Repair may restore exact verified runtime files, owned units/drop-ins and safe
modes, reload systemd, reconcile known missing integration, rerun canonical
idempotent migrations, and restart/verify. It never replaces identity, runs
initdb over data, drops a database, recreates a role blindly, rotates secrets,
changes storage/network, or deletes object content.

**Remove hosted server software** is explicitly data-preserving: stop and
disable only owned services/symlinks, remove owned unit integration and the
replaceable server/PostgreSQL runtimes. Preserve SERVER_CONFIG, SERVER_SECRET,
SERVER_DATABASE/cluster, SERVER_OBJECT_DATA and all identities. Accounts remain
because preserved state owns their IDs. There is no purge in P033. Conversely,
desktop uninstall does not stop or remove hosted-server integration or data.

## Evidence and handoff

Static Rust tests cover foreign/wrong-major identity, system-identifier match,
bounded endpoint reuse, stale plans, and adversarial systemd path escaping.
Network-free validation covers package separation, topology, identities,
credentials, loopback PostgreSQL and preservation. `systemd-analyze verify`
provides host-native syntax evidence; container parsing is not activation.
PostgreSQL acquired by CI is labelled **CI SERVICE FIXTURE**, not a released
artifact. No production PostgreSQL digest is fabricated here.

Known limitation: P034 owns LAN/TLS/reachability, P035 owns first-admin
creation, and P036 owns the complete clean-machine Host journey. Zero users is
therefore a valid P033 infrastructure state.
