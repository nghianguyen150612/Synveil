# Synveil v0.2 server dependency strategy

> Prompt033 implementation note: PostgreSQL 17 now has a distinct immutable runtime/cluster identity and systemd lifecycle described in [SERVER_SERVICE_INSTALLATION](SERVER_SERVICE_INSTALLATION.md); clean-machine acquisition remains P036 and CI packages remain fixtures.

## Status and scope

**Accepted Prompt030 contract.** Personal / Home Mode uses a
**Synveil-managed private PostgreSQL 17 runtime** acquired by the guided Host
flow. Advanced / Server Mode retains explicit **external operator-managed
PostgreSQL 17**. PostgreSQL remains the sole canonical production server
metadata and transaction authority; there is no SQLite production fallback or
second schema model. This document selects ownership and lifecycle policy. It
does not implement provisioning, services, storage, networking, first-admin,
or the final Host wizard and makes no v0.2 readiness claim.

This inherits P029: desktop installation and Connect do not install server
dependencies; Host invokes the separate Server Bootstrap Coordinator. Durable
state is preserved, unknown state fails closed, and uncertain effects are
inspected before retry.

## Current evidence

- ADR-002 selects PostgreSQL. The API and worker require explicit database
  configuration today; no managed Personal / Home database exists.
- `.github/workflows/postgres-17.yml` and numerous ignored live suites qualify
  disposable PostgreSQL **17** databases. Evidence does not qualify 15, 16,
  18, or a future major.
- `MigrationRunner` and ordered migrations are the forward schema authority.
  Unknown/future schema fails closed; neither reset nor binary rollback is a
  substitute for recovery.
- Current deployment is operator-managed. Linux package/service, permission,
  and credential-delivery foundations exist for Ubuntu 24.04 and Fedora 42
  x86_64. Windows native acceptance is still withheld; there is no macOS
  native server/service acceptance.
- Synveil is MIT licensed. Redistribution of PostgreSQL adds separate upstream
  license/notice and provenance duties; this decision is not legal approval
  and does not change Synveil's license.

## Decision matrix

| Concern | Synveil-managed private PG17 runtime **selected** | System-global PostgreSQL | Docker/Compose dependency | External-only PostgreSQL |
|---|---|---|---|---|
| Normal-user UX | Host acquires and prepares a hidden “System database” | User/package-manager/version knowledge leaks through | Requires a container product and daemon | Requires an administrator before Host can work |
| PG17 determinism / distro drift | Exact major, patch artifact, and compatibility are release metadata | Distribution defaults and lifecycle drift | Image can be pinned, but container runtime also varies | Operator can provide PG17; not automatic |
| Isolation / privilege | Private endpoint, cluster, runtime, and restricted identity; bounded setup elevation | Shared service, global settings, ports, roles, and ownership are ambiguous | Container isolation helps but daemon privilege expands the boundary | Isolation and privilege are operator responsibilities |
| Security updates | Synveil explicitly owns verified compatible runtime updates | OS owns package timing and may change support | Image and container runtime both require maintenance | Operator owns server updates |
| Install size/network | Non-zero Host-time download only; desktop/Connect remain small | Often separate OS download | Runtime plus image is largest dependency set | No Synveil payload, but setup cost moves to user |
| Offline possibility | Exact verified cache/bundle/operator-supplied artifact is possible | Repository/media dependent | Preloaded signed images possible but cumbersome | Operator-defined |
| Clean-machine automation | One bounded adapter can acquire, inspect, provision, and reconcile | Varies by distro and enabled repositories | Requires installing/starting another platform | Cannot complete automatically |
| Updates / major upgrade | Same-major replacement is coordinated; future major requires a validated data upgrade | Coupled to OS policy | Image replacement still needs database coordination | Operator coordinates server; Synveil validates compatibility |
| Backup / recovery | Stable ownership permits coordinated DB/object recovery, but automation remains future work | Shared lifecycle complicates ownership | Volumes still need explicit backup/recovery | Operator owns DB backup; Synveil state must still be coordinated |
| Repair / uninstall | Repair owns binaries, never resets data; software removal preserves cluster | Cannot safely repair/remove a shared package | Must distinguish image/container/volume | Synveil cannot repair/remove the server |
| Cross-platform portability | Per-platform artifacts/adapters cost work; initially Linux x86_64 only | Different repositories/services per OS | Broad engine availability, but unacceptable normal-mode prerequisite | Broad where a qualified PG17 endpoint exists |
| Support burden | Synveil assumes packaging, patching, provenance, service, and recovery support | Version/ownership variance produces difficult cases | Adds daemon, image, networking, and volume support | Highest ordinary-user administration burden |

The selected option has material lifecycle cost: Synveil owns release cadence,
security response, provenance, runtime repair, compatibility, and acceptance.
It is selected because it is the only option that combines a clean-machine
ordinary-user flow with deterministic PG17 and unambiguous package/data
ownership. Compose remains a valid Advanced topology and is not deprecated.

## Managed Personal / Home profile

The Host flow acquires a separate, Synveil-owned private PostgreSQL runtime
only after explicit Host consent and preflight. It does not silently reuse a
detected PostgreSQL installation: version, credentials, ports, lifecycle,
upgrade coupling, data ownership, operator expectations, and uninstall
ownership would be ambiguous. Such an installation is external/unowned
infrastructure. Normal mode continues with its private instance.

No Docker, Docker Desktop, Podman, Compose, pre-existing PostgreSQL, arbitrary
OS default version, SaaS database, vendor control plane, telemetry, or hosted
license server is required. The database is local/private, not shared, not a
LAN/Internet endpoint, and never opens a PostgreSQL firewall port. The adapter
may choose a private loopback port other than 5432 or a platform-local endpoint;
the ordinary user never chooses it. Runtime operation after setup needs no
Synveil-operated network service.

Normal UI says **System database**, **Preparing system database**, **System
database ready**, or **System database needs attention**. It never requests or
shows `DATABASE_URL`, `initdb`, `pg_ctl`, a managed password, superuser password,
or raw SQL/libpq errors. Advanced diagnostics may show version, managed/external
mode, health, migrations, artifact identity, redacted data identity, and
connection state—but never secrets or a password-bearing URL.

## Version and compatibility policy

Managed v0.2 and qualified Advanced external v0.2 require PostgreSQL major
**17**. A release records the compatible Synveil server range, managed major,
and dependency artifact version in authenticated metadata; compatibility is
not inferred from a filename or installed executable. Other majors are not
advertised without corresponding qualification.

Verified PG17 security/maintenance releases may replace runtime binaries only
after compatibility preflight, database availability checks, safe stop,
replacement, restart, and health verification. State is never deleted or
recreated. Synveil, not the OS, is responsible for shipping timely compatible
security/runtime updates to its managed dependency. P033 coordinates processes;
P036 proves the path.

A 17-to-future-major change is a database upgrade, not binary replacement.
`pg_upgrade`, dump/restore, or migration to a new cluster requires a separately
validated future decision with recovery evidence. v0.2 performs no automatic
major upgrade. Unsupported major, stale binary/newer schema, or unknown state
fails safely with recovery/operator guidance and never initializes an empty
cluster over data. Arbitrary database binary/schema rollback is not promised.

## Artifact and acquisition contract

`POSTGRESQL_RUNTIME` is a distinct `SERVER_PACKAGE`/managed dependency artifact,
not part of every desktop installer and not `SERVER_DATABASE`. Its authenticated
identity binds component, full upstream PG version and major, platform,
architecture, Synveil packaging/artifact version, SHA-256, source/build
provenance, upstream license/notices, compatibility range, runtime inventory,
and P005 release/P011 channel identity. The future manifest may extend the
existing schema compatibly; P030 does not change production schemas or invent
hashes. Opaque executables, unsigned arbitrary URLs, `latest.zip`, mutable
unpinned downloads, and integrity bypass are forbidden.

Host-time acquisition reuses P005 release identity, P006 trust/integrity, and
P011 selection:

```text
select exact compatible dependency
→ authenticate metadata
→ bounded download
→ verify digest and trust
→ private staging
→ inspect inventory
→ install/provision
→ verify
```

Nothing executes from the download location. A future exact, already-verified
artifact may come from a release cache, offline Host bundle, or operator supply
without changing database semantics. Internet access is neither a permanent
runtime requirement nor a desktop-install requirement.

The artifact lifecycle is Acquire, Verify, Stage, Install, Inspect, Repair,
compatible Update, and Remove-software-preserving-data. It contains no database
or object data, real credential, or machine-generated cluster identity. Its
producer must record upstream source/version, packaging revision,
platform/architecture, build provenance, digest, and required PostgreSQL
licenses/notices. System package-manager primitives are permitted internally
only when identity remains deterministic and no global `postgresql.service` is
silently adopted.

## Runtime, durable data, and initialization authority

These identities are disjoint:

```text
POSTGRESQL_RUNTIME != SERVER_DATABASE != SERVER_CONFIG
                   != SERVER_SECRET != SERVER_OBJECT_DATA
```

Mutable cluster state is never packaged with runtime. The cluster resides in a
protected Synveil-managed platform location, separate from `objects/v1`,
staging, client libraries, and P032's user-selected object root. P031/P033 own
exact paths, ACLs, and identities. It must not be world-readable/writable;
credentials are not placed there for convenience.

After preflight, explicit Host consent, verified dependency, and validated
target ownership, a future adapter is authorized to run `initdb`, initialize a
private cluster, and create roles/database. Directory existence is insufficient
identity and unknown/foreign data is never initialized over. It must reconcile
finite states: **Absent, ArtifactReady, RuntimeReady, ClusterInitialized,
DatabaseReady, NeedsRepair, UnsupportedVersion, External, and Blocked**. It must
also distinguish partial initialization, compatible managed, foreign/unknown,
and corrupt/unavailable clusters. Unknown maps to Blocked, not a wizard page.

If artifact installation, initialization, role creation, or database creation
has an unknown outcome, inspect authoritative state before retry: never blind
second `initdb`, role recreation, or drop/create.

## Roles, secrets, and schema authority

Where practical, separate cluster/bootstrap owner, migration/schema authority,
application runtime role, and maintenance authority. The production API does
not normally hold unrestricted superuser authority. Database credentials are
`SERVER_SECRET`; no superuser password is baked into configuration or plaintext
ordinary environment files. P031 defines generation, protected platform
storage/delivery, rotation, logging redaction, and external administrator input.

Provisioning creates no alternate schema mechanism. The lifecycle is:

```text
database available → validate schema/version → apply canonical ordered forward
migrations through MigrationRunner → verify current schema → start server
```

Unknown/future schema fails closed. There is no reset and no SQLite fallback.

## Advanced external PostgreSQL

Only Advanced / Server Mode offers **Use external PostgreSQL**. The operator
owns PostgreSQL package, service, global configuration, data lifecycle, server
updates, availability, and backup. Synveil may own its database/schema, roles
when explicitly authorized, credentials, and migrations. It must not stop or
upgrade the server, affect unrelated databases, remove the cluster, delete
unrelated data, or change global settings without separate authorization.

Before mutation, later implementation validates PG17 major, connectivity,
TLS/trust policy, capabilities, database identity, permissions/credentials,
migration compatibility, and schema state. “Connection succeeded” alone is
not qualification. Protected administrator configuration may accept external
connection secrets, but ordinary config, UI, diagnostics, and logs may not
expose them.

## Lifecycle, repair, recovery, and preservation

- The eventual runtime must provide controlled start/stop (not reboot as
  success), boot persistence, health inspection, least privilege, repair,
  updates, and data-preserving software removal. Unexpected required reboot is
  Blocked until explicitly supported. P033 selects the service mechanism and
  dedicated restricted identity; desktop/API/PostgreSQL do not run permanently
  as root. Setup elevation is visible, purpose-specific, short-lived, and
  adapter-owned.
- Repair verifies artifact identity, restores managed runtime files, reconciles
  supported service integration, and validates cluster/database availability
  and compatibility. It never initializes over data, drops a database, blindly
  recreates roles, reverses schema, or generates a replacement identity.
- Ordinary desktop uninstall does nothing to the managed dependency. Explicit
  server-software removal may remove runtime/package but preserves
  `SERVER_DATABASE`, `SERVER_CONFIG`, required `SERVER_SECRET`, and
  `SERVER_OBJECT_DATA` by default. Runtime ownership is not cluster ownership.
- Database deletion is a separate destructive operation requiring exact server
  and data identities, explicit server-data deletion, strong confirmation, no
  ambiguity, and owner-specific authorization. There is no “reset PostgreSQL”
  repair action.
- Backup and restore must be possible. Runtime replacement may not impair that
  ability; major upgrades require recovery evidence; database and object data
  are coordinated server state for disaster recovery. The backup roadmap owns
  the future mechanism. Starting successfully is not production readiness.

Managed PostgreSQL has non-zero download, disk, memory, and background-process
cost. P036 must measure and report actual use; P030 promises no benchmark or
minimum RAM.

## Product failures and safe actions

| Category | Normal safe action |
|---|---|
| Dependency acquisition failed | Retry authenticated acquisition, use an exact verified offline artifact, or stop |
| Dependency verification failed | Discard staging; retry trusted metadata/artifact; never bypass |
| System database could not be prepared | Inspect durable state, resume/reconcile, export diagnostics, or stop |
| Existing database requires attention | Do not initialize; inspect identity and obtain administrator guidance |
| Unsupported PostgreSQL version | Stop and use supported recovery/Advanced guidance; never replace data blindly |
| System database unavailable | Retry health/service reconciliation or export redacted diagnostics |
| Managed database needs repair | Repair owned runtime/service integration while preserving cluster |
| External database unavailable | Ask the Advanced operator to restore endpoint/service availability |

Normal UI maps these categories to P010-style safe actions and redacts raw
SQL/libpq errors.

## Platform capability matrix

“Implementation target” is not shipped support; P036 qualification is required.

| Platform | Architecture | Desktop/client | Managed Personal/Home Host | Managed PostgreSQL strategy | Evidence status | Reason |
|---|---|---|---|---|---|---|
| Ubuntu 24.04 | x86_64 | v0.2 target | **Implementation target** | Private Synveil-managed PG17 artifact | Foundations, not Host acceptance | Qualified Linux package/service/security base exists; dependency and end-to-end proof remain |
| Fedora 42 | x86_64 | v0.2 target | **Implementation target** | Private Synveil-managed PG17 artifact | Foundations, not Host acceptance | Same bounded Linux target and remaining gates |
| Windows 11 | x86_64 | Desktop/client target | **NOT_YET_QUALIFIED** | Future private native runtime; no Docker fallback | P028 native acceptance withheld | Client status is unaffected; managed dependency/service proof is absent |
| macOS | any | Future target only | **NOT_YET_QUALIFIED** | Undecided future native runtime | No native package/service/clean-machine evidence | Compilation is not Host qualification |
| Other Linux / architectures | non-x86_64 or other distro | Not established here | **NOT_YET_QUALIFIED** | Requires exact future artifact/adapter | No P036 evidence | Avoid extrapolating from x86_64 packages |

## Security threat ownership

| Threat | Required control and owner (not an implementation claim) |
|---|---|
| Artifact substitution/tampered runtime | P005/P006/P011 authenticated identity, digest, private staging, inventory; Release/Security |
| Weak DB credential / credential logging | Generated protected `SERVER_SECRET`, redaction and delivery; P031 |
| World-readable cluster | Restricted data ACL/identity; P031/P033 |
| Public database listener | Local/private endpoint and no firewall opening; P033; P034 owns only API reachability |
| Reuse of unrelated system cluster | Managed identity validation and explicit Advanced opt-in; P030/P031 |
| Downgrade to vulnerable runtime | Compatibility/channel metadata and anti-downgrade policy; Release/P031/P036 |
| Blind cluster reinitialization | Typed identity, outcome-unknown inspection, fail closed; P031/P033 |
| Privileged-process compromise | Bounded elevation and dedicated restricted service identity; P033/Security |
| Ambiguous cluster ownership | More than directory existence; Blocked state and operator guidance; P031 |
| Database deletion during uninstall | Separate strong-confirmation data deletion; P030/P033/P036 |
| Stale binary/newer schema mismatch | Major/schema preflight and unknown/future fail closed; migrations/P031/P036 |

## Handoffs and non-goals

- **P031** receives managed/external mode, PG17, runtime/data/artifact/endpoint
  ownership, role and secret categories, compatibility policy, typed identity,
  and platform capability. It defines durable config, secrets, permissions,
  reconciliation, and effective runtime configuration.
- **P032** selects `SERVER_OBJECT_DATA`, not raw PostgreSQL cluster layout.
  Whole-server relocation requires an explicit migration design.
- **P033** owns service manager, restricted identities, ordering, persistence,
  health, controlled start/stop, and repair/remove supervision for PG/API/worker.
- **P034** owns API reachability; the PostgreSQL listener stays private.
- **P035** owns first-admin bootstrap and is not consumed here.
- **P036** proves clean-machine acquisition, reconciliation, preservation,
  backup compatibility, platform behavior, and measured resource use. Future
  backup work owns backup/restore automation.

P030 downloads no binary, runs no `initdb`, creates no role/password/service,
changes no bind/migration/schema/product version, and implements no wizard.

## Acceptance requirements

The strategy is locked when ADR-060 is Accepted, OD-PLAT-001 is closed, all
normative documents agree on private managed PG17 versus explicit external
Advanced PG17, runtime and data ownership remain separate, P028 remains pending,
and focused/document validation passes. This is
`SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED`; it is not guided-self-hosting or
Windows-install-experience readiness.
