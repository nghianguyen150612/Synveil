# Synveil v0.2 Server Setup Product Contract

Status: **Accepted product contract — Prompt029; implementation deferred.**

This contract defines what guided self-hosting means to a user and which component owns each server-side effect. It refines the Host/Connect boundary in the [installation product contract](INSTALLATION_PRODUCT_CONTRACT.md) and the [installation ownership architecture](INSTALLATION_ARCHITECTURE.md). It does not describe a shipped guided server installer. The accepted decision is recorded in [ADR-059](../adr/ADR-059-guided-server-setup-product-contract.md).

Normative **MUST**, **MUST NOT**, **SHOULD**, and **MAY** statements apply to P030–P036 and later work that presents or owns guided server setup. This contract does not close the Windows Phase-D native acceptance gate and does not claim v0.2 guided self-hosting is ready.

## 1. Scope and current server reality

P029 locks product semantics and resource ownership. It does not provision PostgreSQL, install or supervise a server, configure storage or networking, create an administrator, add QML, change migrations, or select a concrete dependency strategy. P037 owns the final unified welcome experience; P036 owns the end-to-end Host execution flow.

The implementation remains operator-managed today:

- synveil-api is a Rust/Axum server. PostgreSQL is the canonical metadata and transactional authority; production-style database-backed operation depends on PostgreSQL and the existing ordered migrations.
- When DATABASE_URL is configured, API startup requires SYNVEIL_REBASELINE_TOKEN_KEY, connects to PostgreSQL, and applies the existing migration path. This is current runtime behavior, not an ordinary user installer flow.
- Local object-backed upload and download require an explicit SYNVEIL_OBJECT_ROOT. The API otherwise reports that this backend is unavailable. An enabled synveil-worker also requires PostgreSQL and the configured object root.
- SYNVEIL_BIND_ADDR defaults to 127.0.0.1:3000; the API accepts an optional SYNVEIL_PUBLIC_ORIGIN for public-origin configuration.
- synveil-worker is a separate optional bounded worker with no public listener. Database migrations already exist and remain the canonical forward migration path.
- Desktop packages do not bundle a server stack. Current operator deployment can require PostgreSQL, storage configuration, reverse-proxy/TLS setup, and service supervision.

These facts describe current source and deployment documentation only. They are not proof that any platform already supports guided hosting.

## 2. Product north star

The intended ordinary journey is:

~~~text
Install Synveil
→ launch
→ choose Host Synveil on this device
→ validate this device
→ choose server data location
→ prepare server components
→ create the first owner account
→ verify server health
→ server ready
→ connect this device through the normal client/API profile
~~~

The ordinary user can reach a private usable server without learning infrastructure terminology. Normal setup uses product concepts. Advanced setup may expose infrastructure concepts. Both profiles are the same Synveil protocol, authentication model, PostgreSQL authority, object identity model, migration rules, and durable ownership model; they are not incompatible editions.

## 3. First-run choice: Host or Connect

The conceptual first-run decision has exactly these product choices:

### Host Synveil on this device

This device will run Synveil server components and hold or reference the server's durable data. Host setup is a real server deployment even when the desktop client runs on the same device. Host transfers control to the separate Server Bootstrap Coordinator after the user chooses it and reviews the plan.

Host MUST use the normal versioned API, profile, and authentication boundaries when this device later connects as a client. The desktop MUST NOT open server database files directly, become metadata authority, bypass authentication because the connection is local, or introduce a local-only client protocol.

### Connect to existing Synveil

This device remains a client and connects to a Synveil server that is already running. Connect transfers control to the client connection/onboarding flow owned by P038/P039 and the existing client/profile owners.

Choosing Connect MUST NOT install PostgreSQL, provision server services, create server storage or server credentials, open listening ports, or run server bootstrap. It uses normal origin validation, authentication, credential ownership, and client-library onboarding. A Connect user may choose Host later through an explicit product action if supported.

Choosing or creating a client library and starting file synchronization remain client-owned steps. They are not server-storage setup and are not effects of the Server Bootstrap Coordinator.

The labels and meanings above are product semantics for P037 to represent. P029 does not design or implement the final welcome screen.

## 4. The two setup profiles

### Personal / Home Mode

Personal / Home Mode is the recommended ordinary path for home users, families, desktop users, students, creators, small personal servers, and technically comfortable users who do not want infrastructure administration. Its visible decisions are limited to choices the user owns:

| User decision | Why it is visible |
| --- | --- |
| Host here or Connect to an existing server | Selects whether this device owns a server setup or stays a client. |
| Where Synveil stores server data | Selects the durable server object-data location and its ownership. |
| Who owns the server | Creates the initial Synveil owner/admin account. |
| Whether Synveil should start the hosted server in the background or at sign-in, when P033 cannot apply a safe disclosed default | Controls whether this device provides the host service when the user is not actively in the desktop app. |
| How broadly the server should be reachable, when P034 exposes qualified choices | Reachability changes who can connect and may require explicit platform or network effects. |
| Whether to resume, repair, or stop when attention is needed | Chooses a safe recovery action without implying data deletion. |

Normal mode MUST NOT ask the user to administer PostgreSQL or choose raw service, network, secret-delivery, or runtime implementation details. A safe default may be omitted from the questions only after the owning prompt has validated that default for the current platform and selected profile.

### Advanced / Server Mode

Advanced / Server Mode is for homelab and NAS operators, sysadmins, VPS users, developers, and larger or customized deployments. It may expose categories such as external versus Synveil-managed PostgreSQL, custom storage backends, custom bind/listen configuration, operator-managed TLS/reverse proxy, custom service/supervisor integration, domain/public-origin settings, advanced diagnostics, backup integration, external secrets, and custom resource locations.

This list grants no implementation or support claim. P030–P035 decide exact surfaces and supported values. Advanced choices MUST be real choices with validation, ownership, and consequences explained; a required question does not become optional merely by placing it under an Advanced heading. Advanced mode is not a prerequisite or substitute for a qualified Personal / Home journey.

Both profiles MUST retain the same API semantics, auth model, PostgreSQL authority, object identity model, migration rules, data-safety invariants, and durable ownership model.

## 5. Conceptual setup journeys

### Host — Personal / Home

The ordinary Host flow is:

1. **Check this device** — run read-only preflight and show whether hosting is supported, not yet qualified, or unavailable.
2. **Choose server storage** — ask “Where should Synveil store server data?” and show capacity/availability in user language.
3. **Review what Synveil will manage** — say this device will host Synveil; show the chosen server-data location, background components and startup behavior, and reachability intent; confirm detected data will be inspected and preserved rather than overwritten; and explain that removing the desktop app does not delete hosted data. Raw package names and ports appear only in Advanced mode.
4. **Prepare server** — request consent, then coordinate the approved, bounded effects and truthful progress milestones.
5. **Create your Synveil owner account** — use the normal Synveil auth model; never ask the user to create a PostgreSQL role.
6. **Check server** — verify the complete selected-profile readiness conditions, not just process or port startup.
7. **Ready** — connect this desktop as a normal authenticated client when the selected reachability mode permits it.

The flow MUST be keyboard navigable and screen-reader accessible, use clear focus and standard controls where appropriate, and never communicate status by color alone.

### Host — Advanced / Server

Advanced setup starts with the same Host intent and read-only preflight. It then exposes the applicable infrastructure choices and ownership consequences before any mutation. It uses the same coordinator, safety rules, readiness boundary, and API/auth/data model as Personal / Home Mode.

### Connect

Connect leaves server provisioning entirely and follows the existing/future client connection flow owned by P038/P039. It does not display server provisioning progress. A local Host that completes setup later connects by the same normal versioned API and profile model as every other server.

P029 defines these steps, labels, and meanings only. P037 owns final first-run presentation and P038/P039 own connection/auth UX implementation.

## 6. Progressive disclosure and hidden infrastructure

The normal flow MUST use product language and MUST NOT ask for or expose as user decisions:

- DATABASE_URL, database hostname, port, role, or password;
- PostgreSQL data-directory internals, migration commands, or SQL;
- SYNVEIL_REBASELINE_TOKEN_KEY, SYNVEIL_OBJECT_ROOT, SYNVEIL_BIND_ADDR, or raw environment variables;
- systemd unit names, Windows Service names, raw process supervisors, Docker network/project names, or Compose YAML;
- reverse-proxy/Caddyfile syntax, raw TLS certificate paths, or internal service secrets.

The implementation may use those concepts internally. The ordinary user sees server data location, server preparation, owner account, reachability choice when available, health, and safe recovery actions. The setup MUST NOT ask the user advanced questions and merely conceal their labels.

## 7. Server Bootstrap Coordinator and consent

The Server Bootstrap Coordinator is separate from the Installation Coordinator, desktop GUI lifecycle, synveil-client synchronization lifecycle, and server runtime. Its conceptual lifecycle is:

~~~text
Inspect → Preflight → Plan → Provision requested dependencies → Configure
→ Initialize → Start → Verify → Hand off to first-admin bootstrap → Complete
~~~

It coordinates canonical owners and verifies their effects. It MUST NOT become the canonical owner of every package, secret, database, storage, service, or network resource. It MUST NOT implement file synchronization, client reconciliation, or another sync engine; synveil-client remains the synchronization authority.

Before any privileged or system-level provisioning, the user MUST be told in plain product language that this device will host Synveil, where server data will live, which server components may run in the background, and that removing the desktop app does not delete hosted data. Each privileged action must be explicit, bounded, owned by a platform adapter, and used only when necessary. The desktop application MUST NOT run permanently elevated.

## 8. Host preflight and platform capability

Preflight is read-only and occurs before durable mutation whenever possible. It must establish enough evidence to decide whether the requested Host profile is safe on this device. The owner prompts implement exact probes for:

- operating system, platform/profile support, and architecture;
- required privilege capability and whether the exact approved action can be performed;
- available capacity, selected-location availability, and storage safety;
- relevant port/network conflicts without opening listeners or changing firewall/router state;
- existing Synveil server identity, partial setup, configuration, data, managed database, and service registration;
- resource availability and required dependencies.

Preflight MUST detect enough existing state to avoid provisioning a second server over an existing or ambiguous one. Possible product outcomes include “Existing Synveil server detected”, “Resume setup”, “Repair server”, and “Connect to this server” where those actions are verified and supported. It MUST NOT offer an automatic destructive reset.

Host capability is separate from compilation. Use explicit product states SUPPORTED, UNSUPPORTED, and NOT_YET_QUALIFIED (or an equally closed vocabulary). If the platform cannot host the managed Personal / Home profile, Connect remains available anywhere the client is supported. The ordinary Host flow MUST NOT silently fall back to terminal instructions. An Advanced manual or Compose deployment may be documented for operators, but it is not evidence of Personal / Home acceptance.

## 9. Server identity and resumable state

A durable local server-installation identity must distinguish a new host, an existing Synveil host, partial setup, and foreign/unknown data. It must be versioned, bounded, non-secret, and not inferred solely from directory existence or hostname. It is local setup-management evidence; it does not create a public protocol server ID, replace authenticated origin/TLS trust, or change the existing client profile's server binding. P031/P032 may materialize it; P029 does not select or generate its mechanism.

Durable setup state is a finite typed state, never a wizard page number:

~~~text
NotStarted
PreflightPassed
DependenciesReady
ConfigurationReady
StorageReady
ServicesReady
AdminBootstrapRequired
Ready
NeedsRepair
Blocked
~~~

ServicesReady means the required infrastructure has reached INFRASTRUCTURE_READY; it does not mean the server is ready for ordinary use. AdminBootstrapRequired is a restricted handoff to first-owner creation. Ready requires coherent completed admin bootstrap and the full readiness definition in §17. NeedsRepair preserves data and offers inspect/repair. Blocked means the requested profile cannot safely continue until a stated condition changes or an operator acts. Unknown persisted states fail closed and are reconciled before any retry.

## 10. Resource ownership matrix

Every durable resource has one canonical owner. Executors, verifiers, and the bootstrap coordinator may cooperate without taking ownership from that authority. “Ordinary uninstall” means desktop package removal; a later, separate server-software removal operation must also preserve durable server data by default.

| Resource / ownership class | Canonical owner | Ordinary repair | Ordinary uninstall | Destructive authority | Implementation |
| --- | --- | --- | --- | --- | --- |
| Desktop package | Native package authority / Installation Coordinator | Restore verified desktop payload and owned integration only | Remove desktop payload and its owned integration | Separate exact-scope package purge only | Existing P007–P009; platform prompts |
| APPLICATION_CONFIG / client profiles | User and desktop config owner | Preserve; migrate only through its compatible owner | Preserve | Separate explicit config purge | Existing client owners |
| CREDENTIAL_STATE / client credentials | OS SecretStore adapter | Preserve; change through authenticated flows | Preserve by default | Credential-specific explicit action | Existing auth/credential owners |
| CLIENT_SYNC_STATE / client sync database | synveil-client | Client-owned reconcile/migrate; unknown schema fails closed | Preserve | Separate explicit client-state purge | Existing client-sync owners |
| User library / synchronized files | User; client writes only through authorized sync | Preserve; client safe-onboarding/reconciliation rules apply | Preserve | Never inferred from a non-empty path; separate exact-scope action only | Existing client/storage owners |
| SERVER_PACKAGE | Server dependency/package authority | Verify and restore owned binaries only | Desktop uninstall leaves it alone; explicit server-software removal may remove package payload | Separate server-software removal; never includes durable data by implication | P030 strategy; P033 lifecycle |
| SERVER_RUNTIME | Synveil API/worker runtime; service owner controls process lifecycle | Reconcile/start/stop only through the service owner | Desktop uninstall does not silently stop or remove hosted server runtime | Explicit server-software lifecycle action | P033 |
| SERVER_CONFIG | Host/server administrator; managed writer is its sole mutation path | Validate and reconcile reviewed managed values; never discard unknown state | Preserve | Separate exact-scope server-data action | P031 |
| SERVER_SECRET | Dedicated protected secret owner/boundary | Validate availability and rotate only through an authorized secret workflow | Preserve by default | Secret-specific explicit rotation/removal | P031 |
| SERVER_DATABASE | PostgreSQL authority/operator; application migration runner owns forward schema migration | Restore dependency/configuration health; never reset DB; unknown/future schema fails closed | Preserve | Separate owner-authorized database operation with exact scope | P030, P033, P035 |
| SERVER_OBJECT_DATA | Configured ObjectStore and host/server data owner | Validate availability/identity; repair through verified object-storage operations | Preserve | Separate exact-scope server-data purge; never inferred from root absence | P032 |
| SERVER_SERVICE_INTEGRATION | Platform adapter/service manager that created it | Reconcile this server's recorded integration only | Desktop uninstall does not silently unregister it | Explicit server-software lifecycle action | P033 |
| SERVER_NETWORK_INTEGRATION | Platform/network authority and explicitly authorized adapter | Inspect/reconcile only the recorded, consented integration | Desktop uninstall does not silently alter it | Separate explicit network action | P034 |
| First-admin bootstrap state | One authoritative server auth/bootstrap owner defined by P035 | Resume/reconcile; never create a second competing bootstrap state | Preserve with server database/config | Owner-specific auth action; never generic reset | P035 |
| EXTERNAL_DEPENDENCY (external PostgreSQL, storage, proxy, DNS/TLS) | External operator/platform authority | Inspect as needed; mutate only under separate explicit authorization | Never uninstall or mutate as a side effect | External owner only | P030–P034 as applicable |
| Temporary setup state | Server Bootstrap Coordinator, limited to resumable effect/checkpoint metadata | Reconcile authoritative state before retry; retain unresolved evidence | Retain while incomplete/unknown; clear only after verified completion when no longer needed | No authority over server data; cleanup only its verified temporary state | P030–P036 |

The server package, server config, server secrets, PostgreSQL database, server object data, service integration, network integration, and runtime MUST remain separate ownership classes. They MUST NOT be collapsed into generic “Synveil files”.

## 11. Server storage and client library are different

**Server storage** is where a hosted Synveil server stores canonical object content. It is SERVER_OBJECT_DATA, separate from PostgreSQL metadata, temporary staging, and any client folder. Within server setup, this wording refines P001's broad “Synveil files” language so the server location cannot be confused with the client's synchronized library.

**Client library** is a local folder synchronized by synveil-client for a connected user/library. It is user-owned content and client sync state.

The product MUST NOT label both concepts simply “Synveil folder”. The normal storage question SHOULD be “Where should Synveil store server data?” or equivalent. The normal flow MUST NOT show SYNVEIL_OBJECT_ROOT, objects/v1, staging namespaces, or filesystem-adapter names.

P032 owns actual location discovery/defaults and selection. A validated local filesystem object root may be the ordinary conceptual profile, but P029 does not choose its path. Do not assume the home directory, C:\, a filesystem root, source checkout, or current directory is safe. Advanced storage may later include a custom local path, validated NAS mount, S3-compatible storage, MinIO, or another approved ObjectStore adapter; P029 makes no support claim for those options in v0.2 Normal mode.

| Storage capability | Repository evidence and product classification |
| --- | --- |
| Explicit local filesystem object root | The server storage crate has an IMPLEMENTED/VALIDATED local adapter and the operator-configured API can use it. This does not provide a guided picker, safe default, complete runtime readiness, or managed Host support. A local path is a candidate for Personal / Home after P032 validation. |
| Custom local path / mounted NAS | The local adapter exists, but each path/filesystem must pass ownership, durability, identity, and capability checks. Treat NAS as Advanced/planned until those checks and a platform profile are qualified; mountability alone is not support. |
| S3-compatible storage / MinIO | The repository documents a later adapter and deployment topology, but no current server adapter is claimed here. This is future/Advanced work, not current Personal / Home support. |

If a selected location already contains data, setup must inspect, identify, and validate before mutation. It MUST NOT assume the location is empty, delete contents, overwrite an unknown identity, or initialize a new server over an ambiguous identity. Exact path, capacity, ownership, and identity checks belong to P032. ROOT UNAVAILABLE != DELETE EVERYTHING remains binding.

Normal health language may say “capacity available”, “storage unavailable”, “storage too small”, “storage disconnected”, or “storage needs attention”. It need not explain mount options, inode behavior, fsync, or atomic rename; implementation must validate the required capabilities.

## 12. PostgreSQL remains canonical

PostgreSQL remains Synveil's canonical server metadata and transaction authority in both profiles. P029 does not propose SQLite as the production server database, an embedded JSON database, a second metadata authority, or reuse of client SQLite as server authority. Hiding database infrastructure from ordinary users does not replace its authority.

Normal users do not install PostgreSQL, create database roles/databases, enter DATABASE_URL, or run migrations manually. P030 must define safe dependency discovery/provisioning for supported managed profiles while preserving external PostgreSQL as an Advanced option where implemented. Existing migrations remain ordered, forward-only, and canonical. Unknown/future schema fails closed; binaries changing does not authorize schema rollback or reset.

## 13. Server configuration and secrets

SERVER_CONFIG is durable, non-secret effective server configuration. It may describe storage selection identity, reachability selection, service topology version, feature flags, and configuration schema version. It MUST NOT contain plaintext secrets for convenience. P031 owns concrete schema, generation, permissions, repair, and reconciliation.

SERVER_SECRET has a dedicated protected owner/boundary for database credentials, the rebaseline-token key, Synveil-owned TLS/private key material, and future server secrets. P031 selects concrete protected delivery. Secrets MUST NOT appear in ordinary logs, desktop/QML state, release artifacts, Git, world-readable environment files, or user-facing diagnostic bundles. There is no plaintext credential fallback.

## 14. Service, privilege, and synchronization boundaries

The domain contract is service-manager-neutral. P033 owns API/worker supervision, database-service interaction, startup ordering, least-privilege identities, start/stop/restart, boot startup, repair, and removal behavior. P029 does not choose Windows Services, systemd, task schedulers, or another supervisor.

Privileged operations are explicit, bounded, platform-adapter-owned, and used only where necessary. User input MUST NOT become arbitrary shell commands or string-built privileged commands. The ordinary desktop runtime remains user-level and MUST NOT be elevated permanently.

Server bootstrap may establish API, PostgreSQL, object storage, required worker/service runtime, network readiness, and first-admin readiness. It MUST NOT perform client reconciliation or file synchronization. synveil-client alone remains synchronization authority.

## 15. Network and reachability boundary

Host setup completion does not mean internet-public. The initial safe state may be local-only until P034 validates and applies the user's selected reachability. The existing API loopback default is a safe current baseline.

There MUST be:

- no silent public exposure or implicit 0.0.0.0 bind;
- no mandatory proprietary relay or Synveil-operated cloud control plane;
- no silent firewall hole, router/UPnP forwarding, or network-policy change;
- no certificate-trust bypass, silently disabled certificate validation, or auto-trust of a self-signed certificate without an explicit approved mechanism;
- no plaintext remote authentication for setup convenience.

Remote access requires explicit user/operator intent. P034 owns concrete local/LAN/remote modes, firewall integration, TLS/trust, domain/ACME, and any approved optional relay/coordination choice. Core local/self-hosted operation does not require a Synveil account, Synveil cloud, telemetry service, or proprietary relay.

## 16. First-owner bootstrap boundary

P035 owns first-admin bootstrap semantics, endpoint/UI sequencing, and the single authoritative bootstrap-complete state. Normal product language says “Create your Synveil owner account”, not “Create PostgreSQL user”.

There is no default administrator password, hard-coded credential, anonymous administrator, or logged plaintext password. Bootstrap must have a clear completion boundary and safe retry/resume behavior. Infrastructure may be healthy before first-owner creation, but INFRASTRUCTURE_READY is distinct from ADMIN_BOOTSTRAP_COMPLETE; a server awaiting its first owner is not yet generally ready for normal use. Local hosting uses the same auth and API boundaries and does not bypass authentication.

## 17. Progress, health, and Server Ready

The normal progress vocabulary has stable, truthful milestones:

~~~text
Checking this device
Preparing Synveil
Preparing storage
Preparing system database
Starting Synveil
Creating owner account
Checking server
Ready
~~~

The UI shows a milestone only when its actual evidence supports it and does not expose dozens of implementation substeps. A TCP listener or process start alone is never “Ready”.

**Server Ready** requires all of the following for the selected, supported profile and reachability mode:

1. Required managed dependencies are available and required external dependencies have been verified.
2. Effective server configuration is valid and its protected secret inputs are available without plaintext fallback.
3. PostgreSQL is reachable and every required existing migration is applied successfully; an unknown/future schema is not accepted.
4. The selected object storage is available and identity-valid.
5. The API passes its health/readiness checks.
6. Required service supervision is healthy; optional services are required only when the selected profile enables them.
7. The one authoritative first-admin bootstrap state is coherent and ADMIN_BOOTSTRAP_COMPLETE.
8. The endpoint satisfies the explicit selected reachability mode and normal trust/authentication checks.
9. No known fatal recovery or integrity state remains.

INFRASTRUCTURE_READY may be reported internally before owner creation. Ready for ordinary use is withheld until all nine conditions hold. Server data being stored is distinct from “Backup configured”; storing data on one disk is not described as a backup. P029 does not require off-host backup to reach readiness; product policy may allow operation before one is configured, but health/setup must clearly show that distinction.

Ordinary health may use: **Ready**, **Starting**, **Needs attention**, **Stopped**, **Storage unavailable**, **Database unavailable**, **Configuration problem**, and **Update/recovery required**. Advanced diagnostics may show component health, bounded logs, effective redacted configuration, service states, network endpoint information, and database/storage diagnostic identities separately; diagnostics never contain secret values.

## 18. Failure, cancellation, and recovery

Ordinary failure copy is human-readable and action-oriented, for example:

- “This device can't host Synveil yet.”
- “The selected storage location isn't available.”
- “Synveil couldn't prepare its system database.”
- “Synveil couldn't start the server.”
- “The server is installed but needs attention.”
- “Network setup needs your action.”

Normal UI MUST NOT expose raw database URLs/errors, SQL, systemd exit codes, Windows SCM codes, stack traces, or environment-variable names. Bounded, redacted, component-scoped diagnostics are available under Advanced support.

Before mutation, cancellation is safe and leaves no durable server changes. After provisioning begins, cancellation transitions to a known resumable or recoverable state where possible. The product MUST NOT promise that all external effects roll back. Compensation is effect-specific; failures preserve durable state and offer verified **Resume setup**, **Repair**, or **Leave server data intact** actions where applicable.

If an external/platform action may have succeeded but its confirmation was lost, retain P008/P009 OutcomeUnknown semantics: inspect authoritative state and reconcile by effect identity before retry. This applies to cluster/role creation, service registration, storage initialization, and network integration. Never blindly replay an uncertain effect. If state cannot be established, preserve evidence and stop in NeedsRepair or Blocked.

## 19. Repair, update, uninstall, and destructive actions

Repair reconciles managed dependencies, owned package/service integration, configuration validity, and health. Repair MUST NOT reset PostgreSQL, delete server object data, generate a new identity over existing data, clear users, or erase credentials.

Forward migrations are canonical and ordered. Unknown/future schema fails closed. A managed dependency update coordinates recovery/backup evidence, database compatibility, service stop/start, migration, and health verification under the later owners. It does not roll schema backward just because the binary changed.

Desktop application uninstall and hosted-server lifecycle are separate. Ordinary uninstall MUST preserve SERVER_CONFIG, SERVER_DATABASE, and SERVER_OBJECT_DATA. It must not silently stop/remove server integration as a side effect of removing the desktop package. Any later server-software removal is a separate explicit lifecycle action and preserves durable server data by default. Destructive server-data purge is separate, names exact resources, and requires owner-specific authorization. There is no generic “Reset everything” operation. Retrying setup never authorizes deletion of a user library, server database, server object data, credentials, or external object backend.

## 20. Diagnostics and security invariants

Setup diagnostics are bounded, redacted, useful for support, and component-scoped. They never log database passwords, the rebaseline key, admin passwords, session tokens, credential-file contents, or private-key material. P031/P033 own concrete locations and rotation.

The following invariants are non-negotiable:

- PostgreSQL remains canonical server metadata/transaction authority.
- SecretStore/protected secret-owner boundaries remain authoritative; no plaintext credential fallback.
- No silent public listener or public exposure; no mandatory proprietary relay.
- Least privilege; no arbitrary user-input-derived shell command.
- No trust bypass or plaintext remote-auth shortcut.
- Ordinary uninstall preserves durable server data.
- Unknown outcomes are reconciled before retry; no blind replay or global rollback promise.
- Unknown/future database schema fails closed.
- ROOT UNAVAILABLE != DELETE EVERYTHING and existing non-empty folders are never implicit deletion sets.
- Existing or ambiguous server/storage identity is inspected before mutation.
- Server storage and client library remain distinct ownership concepts.

## 21. P030–P036 decision boundaries

| Prompt | Exclusive implementation/decision scope |
| --- | --- |
| P029 | Server Setup Product Contract — Host/Connect, profiles, ownership, preservation, readiness, recovery, progressive disclosure. |
| P030 | Server Dependency Strategy — compare and choose supported PostgreSQL/runtime dependency provisioning and managed/external boundary. |
| P031 | Managed Server Configuration — durable config generation/schema, protected secrets, permissions, credential delivery, repair/reconciliation. |
| P032 | Storage Location Wizard — selection, defaults, path/capacity/identity/ownership validation, existing-location handling. |
| P033 | Server Service Installation — API/worker/database service integration, startup ordering, least-privilege identities, lifecycle and repair/removal. |
| P034 | Server Network and Reachability Setup — qualified local/LAN/remote choices, listeners, firewall/trust/TLS/domain integration. |
| P035 | Server First-Admin Bootstrap — first-owner creation, secure retry, and the authoritative completion transition. |
| P036 | End-to-End Self-Host Wizard — compose P030–P035 into the verified Host journey and readiness proof. |

These scopes must not be merged. P029 implements none of their runtime work.

### P030 handoff: fixed contract and permitted decisions

P030 receives a closed decision problem. It MUST retain PostgreSQL as canonical; keep manual database administration out of normal mode; safely provision or discover dependencies for each supported managed profile; remain recoverable and data-preserving; and permit an Advanced external-dependency mode where supported.

P030 MAY choose how PostgreSQL and required runtime dependencies are provisioned and versioned on supported profiles, how upgrades work, what privilege/package effects apply, which platforms are feasible, and where the managed/external boundary lies. It must evaluate ordinary-user complexity, platform feasibility, least privilege, upgradeability, backup/restore, PostgreSQL compatibility, database ownership, service lifecycle, offline-install implications, release size, resource footprint, security updates, clean-machine automation, uninstall/data preservation, recovery, and cross-platform maintenance cost. P029 ranks no technologies and does not select a bundled, system, container, portable, or platform-specific PostgreSQL strategy.

## 22. Explicitly deferred decisions

| Open decision | Owner |
| --- | --- |
| Exact PostgreSQL distribution/provisioning, compatibility, dependency versioning/upgrades, and supported managed Host platforms | P030; end-to-end qualification P036 |
| Exact privilege/package strategy and managed versus external database boundary | P030; service identity/supervision P033 |
| Exact server service manager, startup identity/order, repair, and removal | P033 |
| Exact server storage defaults, candidate discovery, location validation, and identity layout | P032 |
| Whether external/S3-compatible storage is supported in v0.2 Advanced or Normal mode | P032, subject to adapter capability and later release qualification |
| TLS, trust, firewall, LAN/remote access, domain/ACME, and optional relay strategy | P034 |
| First-admin concrete API/UI sequencing and retry implementation | P035 |
| Backup automation, destinations, and restore workflow | Future backup roadmap work under docs/en/BACKUP.md; P029 only requires truthful status |
| Server update orchestration across package, service, and migrations | P030 dependency/update policy, P033 service coordination, P036 end-to-end proof |
| Final first-run welcome screen and accessible presentation | P037; Connect setup P038/P039 |

## 23. Acceptance and non-claims

P029 is complete when an engineer can implement P030–P036 without reopening Host versus Connect, Normal versus Advanced, ownership, data preservation, readiness, failure semantics, security boundaries, or progressive disclosure. The contract and its static validation must cover the required product phrases and reject the listed authority, preservation, readiness, exposure, and reconciliation contradictions.

P029 does not claim PostgreSQL provisioning, a server service, a storage wizard, a network setup, first-admin implementation, an end-to-end Host journey, a qualified host platform, or **V0.2 GUIDED SELF-HOSTING READY**. It does not consume P037 or change the independent P028 checkpoint. Phase D native acceptance remains pending and its Windows checkpoint remains withheld.

The only P029 completion marker is:

~~~text
SYNVEIL_SERVER_SETUP_PRODUCT_CONTRACT_LOCKED
~~~

It MUST NOT emit SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY or SYNVEIL_V0_2_WINDOWS_INSTALL_EXPERIENCE_READY.
