# Prompt029 manifest — Server Setup Product Contract

## Baseline and inherited status

| Field | Recorded value |
|---|---|
| Starting authoritative main SHA | 89bdbe6a284f155dbaaccbcb28002474faa985aa |
| Starting branch | main, clean at handoff |
| Working branch | codex/p029-server-setup-product-contract |
| Updated current main | no; origin/main remained at the handoff SHA after fetch/pull |
| Prompt028 inherited status | Phase-D native acceptance remains pending; P028 checkpoint is WITHHELD. P029 does not repair or complete P028. |
| Prompt029 type | Product/architecture contract and documentation validation only |

Phase-D native acceptance remains pending while Phase-E contract work proceeds.
No Windows readiness marker is emitted here.

## Deliverables and reviewed paths

- **docs/v0.2/SERVER_SETUP_PRODUCT_CONTRACT.md** — normative Host/Connect,
  profile, ownership, preservation, readiness, recovery, and handoff contract.
- **docs/adr/ADR-059-guided-server-setup-product-contract.md** — accepted
  bilingual product decision.
- **docs/adr/README.md** — ADR index includes the existing ADR-058 and ADR-059.
- **docs/v0.2/ROADMAP.md** — marks only P029 contract complete; P028 remains
  pending/withheld and P030–P036 remain open.
- **scripts/validate-server-setup-contract.py** — bounded static contract checks.
- **scripts/validate-docs.sh** — invokes the focused P029 contract validator.
- **docs/v0.2/PROMPT029_MANIFEST.md** — this record.

No server implementation, QML, migrations, package/version/tag, release
artifact, or runtime behavior was changed.

## Current server reality audited

The current API is Rust/Axum. PostgreSQL remains canonical. API database mode
requires SYNVEIL_REBASELINE_TOKEN_KEY, applies existing migrations, and
requires DATABASE_URL; local object-backed transfers need explicit
SYNVEIL_OBJECT_ROOT. SYNVEIL_BIND_ADDR defaults to loopback and
SYNVEIL_PUBLIC_ORIGIN participates in public-origin configuration.
synveil-worker is an optional bounded process without a public listener and
requires PostgreSQL/object-root configuration when enabled. Desktop packages
do not bundle a server stack. Operator deployment may require PostgreSQL,
storage configuration, reverse-proxy/TLS, and service supervision.

The API architecture documents a first-admin HTTP subset; this is not a
guided hosting or P035 acceptance claim. Current sources, not planning docs,
were used to distinguish shipped server/runtime behavior from the new target
contract.

## Locked product contract

- **Host:** “Host Synveil on this device” means this device runs server
  components and holds or references durable server data. Host transfers to
  the separate Server Bootstrap Coordinator. A local desktop later connects
  through the normal versioned API, profile, and auth model.
- **Connect:** “Connect to existing Synveil” leaves this device a client. It
  does not install PostgreSQL, provision services/storage/server credentials,
  open listeners, or run server bootstrap.
- **Profiles:** Personal / Home Mode is the recommended ordinary experience.
  Advanced / Server Mode exposes infrastructure categories for experienced
  operators. Both retain the same protocol, auth model, PostgreSQL authority,
  object identity, migrations, and data safety.
- **Ordinary user decisions:** Host vs Connect; server data location; first
  Synveil owner; background/startup behavior only when P033 cannot provide a
  safe disclosed default; qualified reachability when P034 provides it; and
  resume/repair/stop recovery actions.
- **Hidden infrastructure:** PostgreSQL URL/roles/passwords and internals,
  migration/SQL commands, environment variables and internal secrets, raw
  service/supervisor details, Compose/Docker mechanics, raw reverse-proxy/TLS
  configuration.
- **PostgreSQL:** Canonical server metadata and transaction authority in both
  profiles. P029 does not select a distribution/provisioning strategy.
- **Ownership:** The contract defines separate desktop/client owners and
  SERVER_PACKAGE, SERVER_RUNTIME, SERVER_CONFIG, SERVER_SECRET,
  SERVER_DATABASE, SERVER_OBJECT_DATA, SERVER_SERVICE_INTEGRATION,
  SERVER_NETWORK_INTEGRATION, EXTERNAL_DEPENDENCY, first-admin, and temporary
  setup owners. See the normative matrix.
- **Storage:** Server storage is canonical server object data. Client library
  is a user folder synchronized by synveil-client; the product must never
  call both simply “Synveil folder”. Existing or ambiguous data is inspected,
  identified, and validated before mutation.
- **Setup state:** NotStarted, PreflightPassed, DependenciesReady,
  ConfigurationReady, StorageReady, ServicesReady, AdminBootstrapRequired,
  Ready, NeedsRepair, Blocked. It is durable typed state, not page number.
- **Readiness:** PostgreSQL, migrations, selected object storage, config,
  required dependencies/services, API, completed coherent first-admin
  bootstrap, selected reachability/trust, and no fatal recovery state are
  required. TCP listener/process start alone is insufficient. Infra readiness
  and admin-bootstrap completion are distinct.
- **Failure/recovery:** Effect-specific compensation, resumable state,
  authoritative inspection/reconciliation for unknown outcomes, no blind
  replay, no global rollback promise, and no destructive reset.
- **Security/preservation:** No silent public exposure, mandatory proprietary
  relay, trust bypass, plaintext remote auth, plaintext secret fallback,
  arbitrary input-derived shell commands, unknown-schema reset, or
  data-destructive ordinary uninstall. ROOT UNAVAILABLE != DELETE EVERYTHING.
- **Server Bootstrap Coordinator:** Coordinates canonical owners; it does not
  own all resources or client synchronization.
- **Consent:** Before privileged/system-level provisioning, users are told this
  device will host Synveil, where server data will live, which components may
  run in the background, and that removing the desktop app does not delete
  hosted data.

## Prompt boundaries

- P029 — Server Setup Product Contract.
- P030 — Server Dependency Strategy.
- P031 — Managed Server Configuration.
- P032 — Storage Location Wizard.
- P033 — Server Service Installation.
- P034 — Server Network and Reachability Setup.
- P035 — Server First-Admin Bootstrap.
- P036 — End-to-End Self-Host Wizard.

The closed P030 handoff fixes PostgreSQL authority, hidden normal-mode
administration, safe provisioning/discovery, recovery/data preservation, and
the possibility of Advanced external dependencies. P030 chooses supported
managed dependency provisioning/versioning, upgrade strategy,
privilege/package effects, platform feasibility, and managed/external
boundary, evaluating all criteria in the normative contract. P029 ranks no
technologies.

## ADR and roadmap result

- ADR: 059 — docs/adr/ADR-059-guided-server-setup-product-contract.md
- Roadmap: P029 contract complete only. Phase E checkpoint not reached.
  P030–P036 remain open.
- P028 remains Phase-D native evidence pending / checkpoint withheld.
- No SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY or
  SYNVEIL_V0_2_WINDOWS_INSTALL_EXPERIENCE_READY marker.

## Validation performed

- `./scripts/validate-docs.sh` — PASS. DOC-UNIT-1 through DOC-UNIT-8 passed;
  the P029 validator passed all 40 required phrase checks and contradiction
  sentinels.
- `./scripts/validate-install-acceptance.sh` — PASS. Acceptance contract valid
  for 11 scenarios; 21 contract unit tests passed. Native execution was not
  performed.
- `python3 scripts/install_acceptance.py inventory` — PASS, read-only. Schema
  v1 and 11 scenarios were reported; FIRST-RUN-2 remains
  IMPLEMENTATION_PENDING and native execution was not performed.
- `cargo fmt --all -- --check` — PASS.
- `git diff --check` — PASS.
- Cross-document review — PASS. The new contract was compared with the
  installation product/architecture contracts, ADR-049/050, platform,
  deployment, security, storage, backup, API architecture, and roadmap. Its
  server-data terminology explicitly refines P001's broad “Synveil files”
  wording; its local setup identity is not a client protocol/server identity.
  No mature architecture document required an edit.

No native server setup evidence is expected or claimed by P029.

## Open implementation decisions retained

Exact PostgreSQL/runtime distribution and provisioning, supported Host
platforms, upgrade packaging, service manager, storage defaults, v0.2
external/S3 support, TLS/trust, LAN/remote/firewall/domain/ACME strategy,
first-admin implementation sequencing, backup automation, and server update
mechanism remain assigned to P030–P035, P036, or future backup roadmap work as
listed in SERVER_SETUP_PRODUCT_CONTRACT.md.
