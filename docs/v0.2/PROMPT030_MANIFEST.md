# Prompt030 manifest — Server Dependency Strategy

## Baseline and inherited status

| Field | Recorded value |
|---|---|
| Starting authoritative main SHA | `d0c1ed31b1c6384adfbd43e08c67307976c50ecd` |
| Working branch | `codex/p030-server-dependency-strategy` |
| Remote preflight | No `origin` remote is configured in this checkout; the local handoff SHA exactly matched the authoritative SHA. No hosted fetch/push evidence is claimed. |
| P029 | `SYNVEIL_SERVER_SETUP_PRODUCT_CONTRACT_LOCKED` inherited |
| P028 | Windows native acceptance remains pending/WITHHELD; no Phase-D or Windows readiness marker is changed |
| Prompt type | Architecture/strategy and documentation validation only |

## Evidence and prior decision state

ADR-002 keeps PostgreSQL canonical. The production server has no SQLite
fallback. `.github/workflows/postgres-17.yml` and many disposable-database live
integration suites target PostgreSQL 17. `MigrationRunner` and ordered forward
migrations remain schema authority, with unknown/future schema failing closed.
Current deployment requires explicit database configuration; no Personal/Home
managed PostgreSQL implementation exists. OD-PLAT-001 and Proposed ADR-019 had
left distribution open before this prompt.

Repository Linux package, systemd, filesystem-ownership, uninstall, and
credential-delivery foundations support a bounded implementation target, but
are not managed-Host runtime proof. Windows P028 remains pending and no macOS
native package/service acceptance exists. The repository MIT license remains
unchanged; future PostgreSQL redistribution must carry applicable upstream
license/notices and provenance, without this manifest claiming legal approval.

## Locked decision

- Personal/Home selects a **Synveil-managed private PostgreSQL 17 runtime** as a
  distinct Host-time server dependency. Connect and desktop install do not
  acquire it. It does not require Docker or reuse arbitrary system PostgreSQL.
- Advanced/Server retains explicit **external operator-managed PostgreSQL 17**.
  The operator owns its package/service/data lifecycle. Both modes use the same
  PostgreSQL authority, schema, migrations, and application logic.
- Alternatives rejected for normal mode: system-global PostgreSQL because of
  version/ownership/lifecycle ambiguity; Docker/Compose because it introduces
  an unrelated runtime; external-only because it requires administration.
  Compose and external PostgreSQL remain Advanced options.
- `POSTGRESQL_RUNTIME` is `SERVER_PACKAGE`, not `SERVER_DATABASE`. Runtime,
  cluster, config, secrets, and object data have separate identities. Ordinary
  desktop uninstall does nothing; server-software removal preserves durable
  server data by default. Destructive deletion is separately authorized.
- Exact dependency metadata binds component, upstream major/version, platform,
  architecture, artifact/packaging version, SHA-256, provenance,
  license/notices, compatibility, inventory, and release/channel. Acquisition
  reuses P005/P006/P011 selection and trust: authenticated metadata, bounded
  download, verification, private staging, inspection, install, verification.
  No unsigned URL, mutable latest artifact, or bypass is allowed.
- Exact verified cache, future offline bundle, or operator-provided artifact is
  permitted. Internet is not a desktop-install or permanent runtime dependency.
- Managed initialization is authorized only after preflight, Host consent,
  dependency verification, and target ownership validation. Partial, foreign,
  unsupported, corrupt, unavailable, and outcome-unknown states are inspected;
  no blind `initdb`, role recreation, reset, or drop/create.
- Canonical migrations run after database/schema validation. Unknown/future
  schema fails closed; there is no database reset, SQLite fallback, or arbitrary
  database binary rollback.
- Verified compatible PG17 maintenance updates may replace runtime after safe
  stop/restart and health verification without deleting state. Synveil owns
  security update delivery. A future major needs a separately proven database
  upgrade; v0.2 has no automatic major upgrade.
- The listener is private/local and no normal user chooses a database port or
  sees `DATABASE_URL`/passwords. Roles conceptually separate bootstrap owner,
  migration authority, application runtime, and maintenance; P031 owns secrets.

## Platform decision

| Platform | Managed Personal/Home Host state |
|---|---|
| Ubuntu 24.04 x86_64 | Implementation target; P036 qualification still required |
| Fedora 42 x86_64 | Implementation target; P036 qualification still required |
| Windows 11 x86_64 | `NOT_YET_QUALIFIED`; desktop/client target unaffected |
| macOS | `NOT_YET_QUALIFIED` |
| Other Linux/architectures | `NOT_YET_QUALIFIED` |

[ADR-060](../adr/ADR-060-v0.2-managed-postgresql-dependency-strategy.md) is
Accepted and supersedes ADR-019's unresolved distribution portion.
OD-PLAT-001 is Accepted/closed in both PLATFORM documents. OD-PLAT-002 through
OD-PLAT-005 remain open.

## Handoffs

- P031: durable managed/external config, artifact/endpoint identity, secrets,
  permissions, reconciliation, and effective runtime configuration.
- P032: `SERVER_OBJECT_DATA` selection, never raw PostgreSQL cluster layout.
- P033: service manager/identities/order/persistence/health/start-stop and
  repair/removal supervision.
- P034: API reachability only; PostgreSQL remains private.
- P035: first-admin bootstrap.
- P036: clean-machine end-to-end proof, preservation/backup compatibility, and
  measured resource use. Future backup work owns the mechanism.

## Validation and claims

The focused validator checks canonical PG17, managed/external boundaries,
artifact trust, preservation, platform status, ADR/OD closure, and handoffs.
Standard docs, install-acceptance inventory, formatting, and diff checks are
recorded in the PR. No live PostgreSQL provisioning is required or claimed;
documentation checks are not runtime evidence. No artifact, cluster, role,
password, service, schema, migration, Host wizard, version, or tag is changed.

Open work remains P031–P036 and OD-PLAT-002 through OD-PLAT-005. Phase E is
open; this manifest emits neither guided-self-hosting nor Windows readiness.

`SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED`

Prompt030 complete. Next: P031 — Managed Server Configuration.
