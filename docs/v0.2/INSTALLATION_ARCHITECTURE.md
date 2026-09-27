# Synveil v0.2 installation architecture and component ownership

Status: **Normative v0.2 architecture contract — Prompt003; implementation deferred.**
This document refines [ADR-049](../adr/ADR-049-v0.2-effortless-installation-architecture.md)
and is binding on P004 and later implementation prompts. Accepted ownership,
security, migration and released-data compatibility decisions remain binding.

## 1. Architectural boundary

The ordinary installer installs and verifies the desktop/client product. It
does not configure an account, provision a server, or begin synchronization.
After verified installation, first-run runs as the signed-in user and offers
Connect or Host. Host setup belongs to a separate Server Bootstrap Coordinator;
Connect does not acquire server/database components. Existing runtime owners
remain authoritative:

```text
Native installer/package surface
  → Installation Coordinator (common lifecycle policy)
      → platform installation adapter (owned platform effects)
          → native installer / package manager authority

Installed product → first-run setup coordinator (user session)
  ├─ Connect → existing desktop onboarding → synveil-client owns synchronization
  └─ Host → Server Bootstrap Coordinator → server deployment components

synveil-desktop → product UI and local control surface
synveil-client  → synchronization authority and client state
synveil-api     → server API authority
PostgreSQL      → canonical server metadata authority
Object store    → server object-data authority
```

These are responsibility boundaries, not a mandate for a new process, crate,
service, or runtime API. The Installation Coordinator may be code in the native
installer or a shared library in a later implementation, as P007 decides
within this contract. Do not add a component solely to make the diagram
symmetrical.

## 2. Component ownership

| Component | Owns | Does not own |
| --- | --- | --- |
| Product UX | Progress, consent, safe next action, bounded outcome presentation | Package mutation, synchronization, credentials, server data |
| Installation Coordinator | Preflight, plan, lifecycle ordering, policy checks, result classification, verified launch eligibility, repair/uninstall requests, interruption reconciliation | Native OS mechanics, sync, libraries, durable credentials, server provisioning/data, package-manager database |
| Platform installation adapter | Scoped platform effects it creates or is explicitly authorized to reconcile; platform-specific preconditions and verification | Other users' integration, arbitrary OS security policy, package-manager database internals |
| Native installer/package manager | Native package transaction, package registration and package-owned payload according to platform rules | Application config, credentials, client sync state, user libraries, server data |
| First-Run Setup Coordinator | Connect/Host choice and user-level onboarding handoff | Installation transaction or client synchronization |
| Server Bootstrap Coordinator | Host-only acquisition/configuration/readiness of explicitly selected server dependencies | Desktop installation, client synchronization, external dependency administration absent scoped authorization |
| `synveil-desktop` | Product presentation and user-scoped desktop control | Synchronization authority or durable credential storage |
| `synveil-client` | Sync protocol, local sync metadata, library admission and sync lifecycle | Installation and package lifecycle |
| Release metadata authority | Artifact identity, version, platform/architecture, digest and future authenticated trust metadata | Installer effects or user state |

## 3. Common operation contract

Every future operation has an intent, plan, ordered effects, and final result.
The model is conceptual; P007 defines its engine API and P008 defines the
durable transaction journal. Required intents are `Install`, `Upgrade`,
`Repair`, `Uninstall`, and `Verify`. Server bootstrap has its own plan and
result type and is never encoded as an installer intent.

| Concept | Normative meaning |
| --- | --- |
| `InstallationIntent` | Requested lifecycle action and explicit user choices; destructive purge is a distinct, separately confirmed intent. |
| `InstallationPlan` | Read-only preflight findings, ordered proposed effects, ownership, required privilege, preservation set, and expected verification. No mutation occurs while merely planning. |
| `InstallationEffect` | One bounded mutation with stable identity, owner, target scope, preconditions, privilege requirement, mutation description, verification method, retry/reconciliation rule, reversibility class, and preservation constraints. |
| `EffectOwnership` | Exactly one canonical authority from the taxonomy below; executor and verifier may be different collaborators but ownership is singular. |
| `EffectPrecondition` | Version/state/scope facts that must still hold immediately before mutation; stale plans are replanned. |
| `EffectResult` | Verified success, verified no-op, failure before mutation, known partial mutation, or `OutcomeUnknown`; carries redacted evidence and a safe next action. |
| `InstallationResult` | Aggregate lifecycle state and verified completion boundary; cannot turn a package success into Host readiness or synchronization success. |

The coordinator may inspect installation state, construct and present a plan,
request effects from the correct adapter/authority, verify postconditions,
classify failures, request repair/uninstall, reconcile interrupted outcomes,
and launch the desktop only after installation verification and the user's
launch choice. It must not directly write package-manager databases, client
SQLite, credentials, library files, server PostgreSQL/object data, or native
security policy.

Effect execution is ordered and scoped. Before applying an effect, recheck its
preconditions. Verification follows every mutation. A known reversible effect
may be compensated only when its declared inverse is safe and verified. A
partially reversible or irreversible effect is not promised global rollback.
If the result is `OutcomeUnknown`, read authoritative state and reconcile by
effect identity before any replay; if reconciliation cannot establish state,
stop and present recovery/repair. Never blindly repeat an uncertain mutation.

## 4. Canonical resource ownership taxonomy

| Classification | Canonical owner and examples | Upgrade / repair | Ordinary uninstall |
| --- | --- | --- | --- |
| `PACKAGE_OWNED` | Native package authority: executables, bundled Qt/runtime, package icons/docs, install registration and owned manifest files | Replace or restore only verified package-owned paths | Remove package-owned paths using native lifecycle rules |
| `NATIVE_PACKAGE_STATE` | Native OS package manager exclusively: its package database and transaction records | Changed only through native package transactions | Native package manager removal only; installer never edits or deletes it directly |
| `PLATFORM_INTEGRATION_OWNED` | Platform adapter: shortcut/menu entry, launcher registration, current-user startup registration, native uninstall registration it created | Reconcile only this product's recorded integration; preserve explicit disabled autostart | Remove only this product's integration |
| `APPLICATION_CONFIG` | User/product configuration, profiles and preferences in canonical app config locations | Preserve; migrate only through separately authorized compatible migration | Preserve |
| `CREDENTIAL_STATE` | OS SecretStore adapter exclusively | Preserve; credential changes use authenticated product flows | Preserve unless a separately named, explicit credential-removal action is chosen |
| `CLIENT_SYNC_STATE` | `synveil-client`: local database, checkpoints, intents, conflicts, manifests and related durable sync state | Preserve; only client migration/sync recovery authorities may interpret it | Preserve |
| `USER_LIBRARY_CONTENT` | User; synchronized local files and folders | Preserve; never replace as package payload | Preserve; no package removal or purge may infer a deletion set from a non-empty root |
| `SERVER_CONFIG` | Host user/admin; Server Bootstrap Coordinator may mutate only explicitly managed settings | Preserve or update through a reviewed bootstrap migration with preconditions | Preserve |
| `SERVER_DATABASE` | PostgreSQL authority/operator; schema and canonical metadata | Application migrations only under existing guards and verified recovery policy | Preserve; package lifecycle cannot remove or reset it |
| `SERVER_OBJECT_DATA` | Host user/operator and configured object-store authority | Preserve; server-specific verified operations only | Preserve |
| `EXTERNAL_DEPENDENCY` | External operator/native authority: external PostgreSQL, reverse proxy, DNS, TLS endpoint, shared storage | Inspect only as needed; mutate only through a separate explicitly scoped and authorized operation | Never uninstall or mutate as a side effect |
| `EPHEMERAL_RUNTIME_STATE` | Creating runtime/platform owner: sockets, pipes, temporary files, locks and process state | Recreate or clean only when ownership and liveness are verified | Remove only when the owning runtime is stopped and path identity is verified |

Each persistent resource has one canonical owner. A package may contain a
payload that later writes config, but that does not transfer config ownership
to the package. A configured library path is a reference, not package-owned
content. Unknown files or ownership never become deletable merely because they
are adjacent to an install directory.

## 5. Ownership and lifecycle matrix

| Resource | Owner | Created by | Upgrade behavior | Repair behavior | Ordinary uninstall | Destructive purge |
| --- | --- | --- | --- | --- | --- | --- |
| Executables and bundled runtime | `PACKAGE_OWNED` / native package authority | Package installation | Replace from verified artifact | Restore verified package payload | Remove package payload | Same |
| Shortcuts, desktop entry, product startup registration | `PLATFORM_INTEGRATION_OWNED` / adapter | Explicit installation or user choice | Reconcile this product's integration; preserve autostart choice | Recreate missing owned integration only | Remove integration created by this product | Same |
| User configuration and profiles | `APPLICATION_CONFIG` / user | First-run and product | Preserve; compatible app migrations only | Never reset; preserve | Preserve | Delete only after separate exact-scope confirmation and containment validation |
| Durable tokens/credentials | `CREDENTIAL_STATE` / OS SecretStore | Authenticated client flow | Preserve | Do not copy, export or reset | Preserve by default | Separate credential-specific explicit choice only |
| Client SQLite and sync metadata | `CLIENT_SYNC_STATE` / `synveil-client` | Client | Client migration only, unknown/future schema fails closed | No installer writes or reset | Preserve | Only a separately authorized client-state purge with exact validated scope |
| Local library files | `USER_LIBRARY_CONTENT` / user | User/client | Preserve and let client reconcile | Never alter | Preserve | Never inferred; any product purge must name and validate exact target explicitly |
| Host configuration | `SERVER_CONFIG` / host admin + bootstrap owner | Explicit Host setup | Bootstrap coordinator only | Installer repair does not change | Preserve | Explicit host-data operation only |
| PostgreSQL data/schema | `SERVER_DATABASE` / PostgreSQL/operator | PostgreSQL and application migrations | Existing guarded migration and backup rules | Installer repair does not change; no unknown-schema reset | Preserve | Separate database operation, never package purge side effect |
| Server object data | `SERVER_OBJECT_DATA` / operator/object store | Server | Server/object-store owner | Installer repair does not change | Preserve | Separate exact-scope server data operation |
| External services | `EXTERNAL_DEPENDENCY` / external operator | Operator/platform | No implicit mutation | No implicit mutation | No mutation | No mutation |
| Sockets, locks, temporary runtime state | `EPHEMERAL_RUNTIME_STATE` / creator | Runtime | Reconcile liveness and recreate when safe | Clean only proven stale state | Remove owned stale state after stop | Same |
| Native package database | `NATIVE_PACKAGE_STATE` / native OS package manager | Native package manager | Native transaction | Native transaction | Native package removal | Never edit or delete directly |

Credential, sync and library rows remain protected even if physically located
under a user profile. Repair is package/integration repair, never data repair.

## 6. Lifecycle semantics and completion

The common lifecycle is:

```text
Preflight → Plan → [Acquire → VerifyArtifact] → Install → Integrate
→ VerifyInstallation → Complete
```

Acquisition belongs to the release/artifact path and may be omitted for a
locally supplied artifact. Native package managers remain authoritative for
their transaction and database. Completion requires the intended package
payload and required integration to pass verification. Only then may Finish
offer launch. Launching does not mean first-run is complete; Host readiness and
first synchronization are separate results owned by their coordinators.

| Intent | Required semantics |
| --- | --- |
| Install | Confirm target and required consent; verify artifact before mutation; execute owned effects; verify binaries/runtime/integration; then offer launch. Reuse compatible existing user state. |
| Upgrade | Verify supported source/target compatibility and artifact; replace package-owned payload; preserve config, credentials, client state, libraries and all server data; retain native package transaction authority. App/schema migration is separately owned and guarded. |
| Repair | Inspect and report drift; restore missing/corrupt package-owned files and this product's owned integration. Never rewrite/reset user or server state. Unknown state must be preserved and escalated to its canonical owner. |
| Verify | Read-only checks of package and owned integration state. Do not change user choices or repair implicitly. |
| Uninstall | Remove native package payload/registration and only integration created by this product. Stop only product-owned runtime instances through their lifecycle authority. Preserve all durable user/server state. |
| Purge | Distinct, clearly destructive, separately confirmed operation listing exact resource classes and paths. Validate path identity/containment and reject unknown, external, symlink-escaping or shared targets. Purge never expands scope automatically. |

Rollback is per effect. The coordinator can compensate only declared safe,
verified reversible effects. Native package managers may provide their own
transaction guarantees, but Synveil does not infer whole-install rollback from
atomic file replacement. Irreversible or partly reversible work preserves
evidence and moves to reconciliation or a safe repair path.

`ROOT UNAVAILABLE` is a capability/result state, not permission to delete or
reinitialize. Existing non-empty local directories are not deletion manifests.
Unknown/future database schemas fail closed and are never reset. After any
ambiguous mutation, authoritative inspection and reconciliation precede retry;
this includes `OutcomeUnknown` from product control operations.

## 7. Platform adapters and privilege

The common coordinator owns lifecycle policy; each platform adapter owns only
its scoped platform effects and delegates package transactions to the native
mechanism. All adapters provide effect identity, current-state inspection,
preconditions, verification, privilege declaration, and uncertainty handling.

| Surface | Architecture contract |
| --- | --- |
| Windows installer | Target the per-user path without administrator rights for ordinary install. Any future machine-wide option is separate, explicit, and narrowly elevated. Do not choose installer technology here. Install/repair/uninstall touch only package-owned files and current-user integration; no Windows service or global policy changes. |
| DEB/RPM | Package manager owns package database and payload transaction; visible system authorization is expected for system package install/removal. Package hooks remain non-starting and do not enable login autostart. User-scoped runtime supervision remains with the existing user lifecycle. Never run privileged package hooks to edit user data. |
| AppImage | User-level artifact use by default; no root requirement or hidden system installation. Any optional launcher registration is an explicit, adapter-owned current-user effect. Do not mutate system package databases or install services. |
| Quick-install script | Alternate acquisition/front-end only. Authenticated metadata and payload verification precede effects; dispatch into the same plan/effect contract and native authority. It cannot bypass consent, privilege, ownership or reconciliation rules. |

Startup integration is owned by the platform adapter that creates it, under
the product's saved explicit preference and ADR-041. “Start Synveil when I
sign in” is opt-in. Repair/upgrade must preserve a disabled preference and
must not silently re-enable it. GUI and synchronization lifecycles stay
independent; the installer does not own or launch the sync engine as its own
background service.

## 8. First-run and server bootstrap boundary

Installation responsibility ends at verified desktop/client payload and
required package integration. First-run begins on user launch after that
boundary. It reuses existing profiles, and new setup offers Connect or Host.
Connect uses existing desktop/client onboarding and does not require server
artifact acquisition.

Host choice transfers to the Server Bootstrap Coordinator. It owns a separate
resumable plan/result for server component acquisition, host configuration,
database adapter, object root, supervisor, local readiness and explicit
reachability decisions. Its mutations are not installer effects. PostgreSQL
remains canonical metadata authority; managed and external dependencies are
explicitly distinguished. An external database, reverse proxy, DNS, TLS or
operator service is never silently taken over. Managed PostgreSQL distribution
and service privilege remain deferred to the named ADR-019/roadmap decisions;
this contract does not accept ADR-019 or ADR-020.

Bootstrap failure does not roll back desktop installation, erase state, reset
a database, or imply sync failure/success. A successful server bootstrap is
not synchronization: `synveil-client` alone owns client synchronization.
Public reachability is never enabled implicitly.

## 9. Release trust, diagnostics and forbidden mutations

Release/Artifact Metadata is the authority for artifact identity, version,
platform/architecture, digest and future signature/trust data. P003 does not
define its format or implementation. The installer verifies the selected
artifact against trusted metadata before execution; a checksum delivered only
by the same untrusted source is not authentication. P005/P006 own the concrete
metadata and download contracts.

Diagnostics are finite, bounded and redacted. They may identify effect IDs,
phases and safe error categories but must not expose tokens, credentials, raw
secret-store values, arbitrary library paths or unsanitized server settings.

Forbidden cross-component mutations:

- Installer, first-run and server bootstrap never own synchronization;
  desktop presents controls while `synveil-client` remains sync authority.
- Installer cannot directly modify client SQLite or infer sync metadata.
- Desktop configuration is not credential storage; the OS SecretStore owns
  durable credentials.
- Package removal cannot delete a library or server object data.
- Unknown/future database schema cannot be reset to make install succeed.
- An unknown operation outcome cannot trigger blind replay.
- ROOT UNAVAILABLE cannot authorize deletion or broad reset.
- Desktop control remains user-scoped local IPC; no public TCP/HTTP control
  listener is introduced.
- Durable client-state mutation remains authoritative before any runtime
  signal/wake; a lost or coalesced signal never rolls back durable state.
- Package hooks cannot silently enable autostart or start persistent client
  supervision.
- Ordinary uninstall preserves user, client and server durable state.

## 10. Prompt002 blocker disposition

| ID | Architecture disposition | Implementation / acceptance ownership |
| --- | --- | --- |
| INS-01 | ARCHITECTURE_RESOLVED — package, per-user Windows adapter and verification boundary defined | P021–P028; native installed-app acceptance P028/P045 |
| INS-02 | ARCHITECTURE_RESOLVED — DEB/RPM native authority and shared lifecycle model defined | P012–P014, P020; acceptance P004/P020 |
| INS-03 | ARCHITECTURE_RESOLVED — authenticated release selection is separate from effects; quick-install dispatches through common contract | P005–P006, P017–P018 |
| INS-04 | ARCHITECTURE_RESOLVED — AppImage is user-level integration adapter with shared ownership rules | P005, P015–P016, P020 |
| INS-05 | ARCHITECTURE_RESOLVED — opt-in platform integration preserves explicit disabled state | P007, P012–P014, P019, P026 |
| INS-06 | ARCHITECTURE_RESOLVED — common intents/effects, verification, reconciliation and effect-specific compensation defined | P007–P010, P020, P027–P028 |
| INS-07 | ARCHITECTURE_RESOLVED — server provisioning belongs to separate Host bootstrap domain | P029–P034; native server delivery P030–P036 |
| INS-08 | ARCHITECTURE_RESOLVED — database, secrets, storage, reachability and supervision are scoped bootstrap effects; implementation choices remain deferred | P030–P036, with ADR-019/020 decision gates |
| INS-09 | ARCHITECTURE_RESOLVED — uninstall removes owned integration/payload and preserves user/server state; purge is separate | P009, P027 |
| INS-10 | ACCEPTANCE_PENDING — architecture cannot establish clean-host native evidence | P004; native acceptance P020, P028, P036, P045 |

These statuses resolve architecture aspects only. No Prompt002 blocker is
implemented or claimed delivered by P003.

## 11. Decision and prompt boundaries

P004 owns deterministic clean-machine scenarios, preconditions and assertions;
it must not implement installer behavior. P005/P006 own concrete artifact
metadata, trust and download formats. P007 owns the common engine; P008 owns
durable transaction journaling; P009 owns lifecycle contract details; P010
owns user-facing error mapping. Platform implementation and acceptance remain
with the named Windows/Linux/AppImage prompts in the roadmap. P029–P036 own
server/bootstrap design and implementation within ADR-019/020 gates.

Future decisions remain with their existing owners: P021/P030/P034 and the
corresponding accepted ADRs determine migration, managed PostgreSQL, service
identity/supervision, reachability and platform-specific runtime behavior.
This contract does not choose Windows installer technology, a PostgreSQL
distribution, a privileged Host service, relay/access technology, artifact
manifest schema, release-signature scheme, or implementation crate layout.

The contract preserves the frozen v0.1 baseline: product version `0.1.0`, 36
server migrations, 7 client migrations and `LOCAL_SCHEMA_VERSION = 7`. It adds
no runtime, schema, migration, dependency, artifact or release claim.
