# Prompt033 evidence manifest — managed server services

- Starting main: `1176f31341b2af286768468160eb5e735076b886`
- Branch: `codex/p033-server-service-installation`
- Inherited markers: `SYNVEIL_SERVER_SETUP_PRODUCT_CONTRACT_LOCKED`, `SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED`, `SYNVEIL_MANAGED_SERVER_CONFIGURATION_READY`, `SYNVEIL_SERVER_STORAGE_LOCATION_READY`.
- P028 Windows native acceptance remains pending.

## Locked implementation

Topology is `synveil-server.target` → private `synveil-postgresql.service` →
`synveil-server-migrate.service` → API → worker. Machine boot enablement uses
`synveil` for applications and separate `synveil-postgres` for database files.
The distinct server manifest owns three closed runtime siblings beneath
`/opt/synveil/server/<runtime-id>`; the desktop owns none. PostgreSQL runtime is
independently verified beneath `/opt/synveil/postgresql/17/<runtime-id>` and its
cluster remains durable at `/var/lib/synveil/postgresql/17/data`.

The non-secret V1 `.synveil-managed-postgresql.json` marker binds server UUIDv7,
stable cluster UUIDv7, PostgreSQL 17, runtime ID, data directory, and numeric
system identifier. Bounded loopback ports are 55432–55463; persisted selection
wins and conflict fails closed. Application role/database are `synveil` without
cluster-global privileges. P031's password is reused; URL remains secret.

Canonical `MigrationRunner` is the migration one-shot. API gets database-url
and rebaseline-token-key; worker/migration get database-url, all through
`LoadCredential`. Storage handoff requires verified P032 identity/closed layout,
escaped exact `ReadWritePaths`, and never recursive chown.

Repair replaces verified software/integration only. Removal disables owned
integration and removes replaceable runtimes while preserving config, secrets,
database/cluster, object storage, and accounts. Desktop uninstall is isolated.
AdvancedExternal never manages PostgreSQL.

## Evidence classification

Rust tests cover cluster/system identity, bounded endpoint and adversarial path
escaping. Static validation covers topology, credentials, sandbox,
sysusers/tmpfiles, package separation and private PG. Native systemd evidence is
claimed only when run. PostgreSQL from CI is a **CI SERVICE FIXTURE**, not a
production artifact; no release digest is claimed. Fedora container parsing is
not systemd activation.

P034 reachability, P035 first admin, and P036 guided acceptance remain deferred.
FIRST-RUN-2 stays `IMPLEMENTATION_PENDING`; this is infrastructure readiness.
