# P001 — Synveil v0.2 release and repository baseline audit

**Evidence captured:** 2026-10-23 UTC
**Repository:** [`nghianguyen150612/Synveil`](https://github.com/nghianguyen150612/Synveil)
**Base:** `origin/develop/v0.3` at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`
**Audit branch:** `audit/v03-p001-baseline`
**Audit scope:** read-only investigation plus audit documentation and a non-production artifact validator. No product behavior, database, migration, integration, release or installer source was changed.

## Executive result

The repository and branch baseline were verified and this audit records the current v0.2 source, release evidence, interfaces and limitations. The official v0.2 release state is **BLOCKED**. Version `0.2.0` is present in source and exact-source CI candidates exist, but there is no `v0.2.0` tag, GitHub Release, signed public artifact set, activated stable channel, completed P047 qualification or P048 approval. The candidate artifacts are CI evidence, not a released product.

The checked-in roadmap `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not present anywhere under `/workspace` or in the repository at collection time. No missing roadmap rules were inferred; P001 was performed against the requirements in its prompt only. This is a context limitation, not a claim that the roadmap does not exist elsewhere.

The repository contains established Rust server and desktop components, a React web surface, Android and iOS client implementations, API v1 contracts, and several independent persistent-state systems. Compatibility is not certified across released v0.2 clients because v0.2 has not been released and no supported cross-version matrix was found.

## Classification key

| Classification | Use in this report |
|---|---|
| `VERIFIED` | The stated narrow claim is supported by a recorded test, CI run, Git ref or artifact record. It does not certify broader production readiness. |
| `IMPLEMENTED_UNVERIFIED` | Source or configuration implements the behavior, but release-grade or target-environment evidence is absent. |
| `PARTIAL` | Some layers or platforms implement the capability; product, client or acceptance coverage is incomplete. |
| `BLOCKED` | A required input, approval, artifact, environment or gate is absent or failed. |
| `MISSING` | No implementation was found in the inspected source/configuration. |
| `UNKNOWN` | Available repository and external evidence does not establish the claim. |
| `NOT_APPLICABLE` | The platform/capability is not in the documented v0.2 target set. |

## Git preflight and immutable baseline

| Item | Observed result |
|---|---|
| Actual repository | `/workspace/Synveil`, remote `origin=https://github.com/nghianguyen150612/Synveil.git` |
| Initial working tree | Clean; branch `work`, no staged or unstaged user changes |
| `origin/main` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` |
| `origin/develop/v0.3` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` |
| Merge base / divergence | Merge base equals both refs; `0 0` commits diverged; initial `HEAD` equals both |
| Working branch | Created as `audit/v03-p001-baseline` from verified `origin/develop/v0.3` after confirming a clean tree |
| Existing target branch | No local or remote `audit/v03-p001-baseline` existed at preflight |
| Fetch configuration | The local origin refspec tracked `main` only; `develop/v0.3` was fetched explicitly without rewriting history |
| Current branch protection | GitHub lists `main` and `develop/v0.3` as unprotected; no required-check policy is asserted from inaccessible settings |

Git evidence: [main](https://github.com/nghianguyen150612/Synveil/commit/604499fce0dfd134d82da3d5bb5d4a055c5eea8d), [develop/v0.3](https://github.com/nghianguyen150612/Synveil/tree/develop/v0.3), and the local preflight commands recorded in [PROMPT001_MANIFEST.md](../manifests/PROMPT001_MANIFEST.md).

At preflight the GitHub Releases endpoint returned an empty list and the open PR endpoint returned an empty list. `git ls-remote --tags origin` showed only the annotated `v0.1.0` tag, peeled to `fa23232ff0154f627ebdd221ec5435134f177af0`; there was no `v0.2.0` tag. The top commit at both main and develop is the merge of PR #155, `docs(release): record R05 blocked qualification handoff`.

## Audit A — v0.2 release state

### Current disposition

**`BLOCKED` — v0.2.0 is not a qualified or published stable release.** `Cargo.toml` and the workspace crates identify the product source as `0.2.0`. The repository README and release records identify v0.1.0 as the current released product. The `v0.1.0` Git tag exists, but GitHub currently lists no Releases. Neither a tag nor a GitHub Release for v0.2.0 exists.

### R01–R05 integration and release evidence

The R05 release manifest records the integrated history: R01 PR #148 (`d8b5d3e7ec00e6992a44de6cee3b0af799667af3`), R02 PR #149 (`ac8c3e5c7c5841baa5f55029bd9c77a1861f5ea1`), R03 PR #150 (`df6afc2d7bba4ee063946f60f0bba9e067e487e0`), R04 PR #151 (`fc23d4d356ba746d739feed30e0b5aa144f0c87b`), and R05 PRs #152 (`d0b1c43c144493e4adf9b44d3fd24d02f2bd7dbb`), #153 (`503920aeac5f087f78fea253192579b71ae5e620`) and #154 (`deef2db375764d3157afb5371eb963bbd9b3f1ea`). PR #155 added the current blocked qualification handoff at baseline SHA `604499f…`.

R05's source manifest and qualification report contain a 2026-10-20 05:44 UTC snapshot that said some jobs were still running. Current GitHub run/job evidence is later than that snapshot and supersedes those particular pending statuses:

| Evidence | Current evidence at collection |
|---|---|
| Exact-source release candidate | [Run 39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679) completed successfully on source `deef2db375764d3157afb5371eb963bbd9b3f1ea` with `sign_candidate=false`. It assembled a valid `VERIFIED_UNSIGNED_CANDIDATE` manifest, SHA-256 `e3a5755b4b036105a2b9f8baa5ffd9348656bcc8aa2b07b38e11247afa3eb2c9`. |
| Candidate archive | GitHub artifact `synveil-release-candidate`, artifact ID `12245434492`, 166,869,997 bytes; Actions ZIP digest `sha256:1d0fe6d38d8d4362fc4306aa400bc98f436939f00f32b82a0177534671f9d5e9`; expiration `2026-11-03T06:42:19Z`. The archive is temporary CI retention, not a public release. |
| Exact-source platform bytes | The assembled manifest records the five outputs below, all for version `0.2.0` and source `deef2db…`. Hashes are candidate-byte hashes, not signatures. |
| Independent Windows ZIP producers | Windows installer run [39377172994](https://github.com/nghianguyen150612/Synveil/actions/runs/39377172994) and Rust CI run [39377177309](https://github.com/nghianguyen150612/Synveil/actions/runs/39377177309) both report portable ZIP SHA-256 `86bc0560b646093888a565418f10664d7ccddc90d63633a29711a06c26359c06`, 51,643,609 bytes. The earlier R03/R05 mismatch is resolved for these exact-source builds. Both builders independently reproduced their own ZIP. |
| Linux native packages | [Run 39377178170](https://github.com/nghianguyen150612/Synveil/actions/runs/39377178170) completed with DEB/RPM build and validation, Qt reproducibility diagnostics, APT install/non-root launch/removal/preservation and Fedora 42 userspace DNF install/non-root launch/erase/preservation jobs successful. These jobs are bounded CI/package evidence, not a native clean-machine graphical release acceptance. |
| PostgreSQL 17 | [Run 39377173109](https://github.com/nghianguyen150612/Synveil/actions/runs/39377173109) completed successfully. Its live suite includes the migration gate applying all 36 current PostgreSQL migrations to a fresh database. It does not prove a production upgrade from an earlier release. |
| AppImage | [Run 39373764291](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764291) passed exact-source build, reproducibility, manifest/runtime inspection, QML smoke and current-user lifecycle checks on hosted Ubuntu 22.04. This does not establish Ubuntu 24.04/Fedora 42 clean-machine GUI acceptance. |
| iOS | Exact-source [static validation 39373790719](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790719), [Xcode build 39373790899](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790899), [Simulator tests 39373790837](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790837), and [Rust Apple artifact build 39373790728](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790728) passed. These are static, Xcode/Simulator and Rust artifact results, not physical-device, live-server, signing or distribution evidence. |
| Windows clean-machine/native gate | [Run 39373764284](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764284) ended with the expected environment block: standard-user Setup acceptance passed on hosted Windows Server 2025; Windows 11 interactive UI, real logon/restart, named-pipe and controlled-server journeys were not established. |
| Managed Host / P036 | [Run 39373791245](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245) failed at approved public publisher-keyring materialization; native host and interruption jobs were cancelled. The production eight-entry inventory and detached signature are absent. |

### Exact-source unsigned candidate manifest

| Platform artifact | Bytes | SHA-256 | Candidate classification |
|---|---:|---|---|
| `Synveil-0.2.0-x86_64.AppImage` | 49,183,224 | `746485e81835a885037d91b2bbe6cfaebc09741caa0f987b6dc60bdbde3833b1` | `IMPLEMENTED_UNVERIFIED` for release; reproducible candidate only |
| `synveil_0.2.0_amd64.deb` | 17,567,768 | `76b79b7f579e2638fadf5ca1a5b6637de8c12e7553ac16bba32a173f2c247ed6` | `IMPLEMENTED_UNVERIFIED` for release; reproducible candidate only |
| `synveil-0.2.0-1.x86_64.rpm` | 17,582,692 | `11aa1f2a54c3b65c88250a07d00d2589d9b4c5fe1f6528b0034bdf9609de7d02` | `IMPLEMENTED_UNVERIFIED` for release; reproducible candidate only |
| `SynveilSetup.exe` | 32,816,689 | `89f500d5f806e41695d1307e1cda342f361fc9c24abd865a7c44cdccfef1d57a` | `IMPLEMENTED_UNVERIFIED` for release; unsigned Windows candidate |
| `synveil-0.2.0-windows-x86_64.zip` | 51,643,609 | `86bc0560b646093888a565418f10664d7ccddc90d63633a29711a06c26359c06` | `IMPLEMENTED_UNVERIFIED` for release; reproducible unsigned candidate |

The five artifact hashes and source identity are taken from the assembled manifest output in run 39373799679. The Action artifact archive has a separate digest. A CI checksum establishes byte identity only; it does not authenticate Synveil's publisher or make an artifact distributable.

### Unsatisfied release gates

* **P047/P048 `BLOCKED`:** the R05 qualification record has 21 gates; mandatory production and acceptance gates remain unmet. P048 approval is not recorded.
* **Production signing `BLOCKED`:** no authorized Authenticode certificate/thumbprint/timestamp evidence, RPM/OpenPGP signing identity and independent fingerprint, or protected Ed25519 release-metadata key and freshness/rollback policy was used. Candidate pipeline signing jobs were skipped.
* **P036 production inventory `BLOCKED`:** `acceptance/p036/production-artifacts.json` and `.sig` are absent. The accepted inventory requires actual authenticated desktop/server/PostgreSQL/Caddy inputs, source and payload provenance, complete notices, independently trusted fingerprints and validation.
* **Native qualification `BLOCKED`:** Windows native GUI/logon and server-backed journeys are unavailable on the hosted Windows Server runner. No exact-source Linux clean-machine consumer run was found for Ubuntu 24.04/Fedora 42. AppImage and package smoke evidence is narrower than those gates.
* **Upgrade/recovery `BLOCKED`:** `deploy/release/native-upgrade-policy.json` and `windows-upgrade-policy.json` both declare `upgrade_from: []`. No supported v0.1.0-to-v0.2.0 upgrade, downgrade, database rollback or power-loss recovery is qualified.
* **Distribution `BLOCKED`:** no stable channel, authenticated public artifact origin, public release links, `v0.2.0` tag or GitHub Release exists.

Current `HEAD` is `604499f…`, while the assembled candidate is for `deef2db…`. `git diff deef2db… 604499f…` shows six release/readiness documentation files only; no product/runtime source changed in that range. This does not turn the unsigned `deef2db…` artifacts into qualified artifacts for the later commit. No release has been created by this audit.

## Audit B — repository architecture and contracts

### Rust workspace and service boundaries

The root `Cargo.toml` defines 18 workspace crates (edition 2024, version 0.2.0):

| Component | Existing responsibility |
|---|---|
| `synveil-core` | Domain IDs/value types, errors, hashes, sync and backup domain contracts. |
| `synveil-auth` | Password, browser-session and device-credential services behind application boundaries. |
| `synveil-metadata` | PostgreSQL pool/repositories, migrations, ownership and durable metadata state. |
| `synveil-object-store`, `synveil-storage` | Opaque object-store interface and staged upload/download/integrity lifecycle; object bytes are separate from PostgreSQL metadata. |
| `synveil-api` | Axum HTTP routes, request/response contracts, authentication/CSRF middleware, API/server/worker/migration binaries. |
| `synveil-server-config`, `synveil-server-network`, `synveil-server-service`, `synveil-server-bootstrap`, `synveil-server-host` | Validated config, local/LAN reachability planning, service lifecycle, first-admin/bootstrap and Linux host adapters. |
| `synveil-client-sync` | Desktop client durable sync and recovery domain; SQLite-backed local profiles, checkpoints, queue and conflict evidence. |
| `synveil-client` | Background client process, lifecycle, platform networking, local control IPC and controller. |
| `synveil-desktop` | Qt/QML UI, tray and control surface; client remains a separate process. |
| `synveil-platform` | Runtime paths, lifecycle and OS-backed credential-store abstraction. |
| `synveil-install-engine` | Installer plans, lifecycle model, transaction journal, error model and platform adapters. |
| `synveil-ios-ffi` | Narrow C ABI used by the Swift Rust bridge. |

`apps/web` is React/TypeScript/Vite. `clients/android` is a Kotlin/Jetpack Compose Gradle project. `clients/ios` is a SwiftUI/Xcode project. `services/ai` is a Python package; it is not part of the Rust workspace. These clients use HTTP/API contracts and do not own server database or object-store access.

### HTTP, identity and storage boundaries

* `crates/api/src/router.rs` mounts product routes under `/api/v1`; health probes also have unversioned deployment paths. `api/openapi.yaml` describes the route contract, but its `info.version` remains `0.1.0-blueprint` while source packages are `0.2.0`. The route prefix is the observed wire-version boundary; spec metadata/version alignment is **UNKNOWN**.
* Routes cover bootstrap/login/session/CSRF/logout, device enrollment/revocation, libraries/nodes/versions, resumable uploads/content reads, device sync/rebaseline/mutations/conflicts, and backup read/maintenance/restore/prune operations.
* Authenticated requests distinguish `BrowserSession` from `DeviceCredential`; the API does not pretend a device is a browser user. Resource methods scope records to owner, library and/or registered device.
* PostgreSQL owns canonical identity and metadata. ObjectStore owns bytes behind server-generated opaque keys. Logical IDs do not authorize a read and clients never receive physical storage keys or paths.
* `synveil-client-sync` owns durable desktop sync state; the desktop UI communicates with the client process over local control IPC. The public API is not a desktop control listener.
* Server API/worker, migration command, and scheduled-maintenance command are separate binaries. The worker is internal and has no public listener.
* The API supports TLS via the deployment edge. The managed Host network planner binds API backend to `127.0.0.1:3000`, uses HTTPS on `443`, and offers local-only/private-LAN modes. Operator-managed HTTPS remains a separate mode.
* A source search found no Tailscale client/control-plane integration. `crates/server-host/src/native/network.rs` classifies interface names beginning `tailscale` as `Tunnel` and excludes tunnels from its private-LAN candidates; that is existing network detection behavior, not SynveilNet support.

### Future v0.3 integration points found in the current design

These are source boundaries to preserve, not new architecture proposals: installer lifecycle and journal in `synveil-install-engine`; OS-specific runtime/secrets and paths in `synveil-platform`; managed Host/network plans and service installation in `synveil-server-host`/`server-network`/`server-service`; authentication ports and `AuthenticatedPrincipal` in `synveil-auth`/`synveil-api`; owner/device-scoped persistence in `synveil-metadata`; `/api/v1` and OpenAPI contracts; object integrity and opaque storage abstraction in `synveil-storage`/`object-store`; and independent desktop, Android and iOS client persistence/transport layers. No general RBAC, approval queue, Protected Areas model, passkey/FIDO2 flow, Synveil CLI, network mount or protected push implementation was found in product source.

## Audit C — installer and deployment

### Existing install and first-run paths

| Platform/deployment | Observed v0.2 source/configuration | Evidence status |
|---|---|---|
| Linux desktop | DEB/RPM packages, package-neutral manifest engine, authenticated quick-install/channel client, AppImage acquisition/integration; package install uses the native package manager and ordinary-user installer. Candidate artifact CI and bounded package jobs exist. | `IMPLEMENTED_UNVERIFIED`; no public stable distribution, production signing or P045 clean-machine GUI pass. |
| Windows desktop | Inno Setup per-user Setup, portable ZIP, closed payload/ownership manifest, same-version repair/uninstall lifecycle and non-admin product operations. | `IMPLEMENTED_UNVERIFIED`; hosted standard-user subtest passes, but interactive Windows 11 acceptance is blocked. |
| macOS desktop | Workspace compile/test targets exist, but no documented v0.2 macOS desktop artifact or supported install path was found. | `NOT_APPLICABLE` to the documented v0.2 desktop target set. |
| Linux server/Host | Released v0.1 operational docs describe Linux, PostgreSQL, durable object root and operator-managed HTTPS/service supervision. v0.2 contains a systemd/server manifest and host/network installation code. Server input bundles exclude PostgreSQL and Caddy; exact production inputs and P036 inventory are absent. | Released baseline documented; v0.2 managed Host release `BLOCKED`. |
| Web first run | `/api/v1/system/bootstrap-status`, one-time `/api/v1/bootstrap/admin`, then login/session; React Setup/Login surface. This is account bootstrap, not a machine installer. | Source implemented; release-grade cross-deployment evidence incomplete. |
| Android/iOS setup | Server profile, device enrollment/auth and app first-run flows are platform UI, separate from host installation. | Client source present; mobile distribution/release evidence blocked. |

The product does not contain the requested future three-step Installer Wizard as a completed feature. Existing integration surfaces are `synveil-install-engine` planner/journal/lifecycle; Linux package and AppImage adapters; Windows per-user Inno Setup; `deploy/install` and `deploy/server` closed manifests; `server-host` service and reachability plans; and server bootstrap. Keep first-admin creation distinct from installing/configuring a server.

### Prerequisites, state and recovery

* Linux package install/upgrade/removal delegates to APT/DNF/RPM/DEB; privilege elevation is scoped to the native package operation. AppImage is user-level. Windows Setup targets current-user install and does not run the product as administrator. The exact supported OS/distro list is gated by native acceptance, not by a successful cross-build.
* Desktop credentials are stored through the OS credential abstraction (Linux Secret Service, Windows Credential Manager); unsupported backends have no disk fallback. Server DB connection/configuration credentials are separately managed runtime inputs. Client profiles/configuration and server objects are durable state and must not be inferred from install-owned files.
* Linux ordinary package removal retains `/var/lib/synveil`, `/etc/synveil`, credential files and external object storage; Windows lifecycle ownership separates package files/registry from user data and unknown neighbor files. Explicit purge is a separate destructive operation.
* Installation resilience code uses an append-only journal, staged plans and reconciliation for ambiguous effects. Synthetic process-interruption and bounded install/erase fixtures exist. They do not establish host power-loss, full native first-run, supported upgrade, database rollback or end-to-end data preservation.
* `upgrade_from: []` on native and Windows policy prevents an advertised v0.1.0-to-v0.2.0 upgrade. Same-version repair behavior is distinct from a supported upgrade. No schema downgrade path is present.
* No production installer workflow or successful v0.2 upgrade/recovery is promoted from CI packaging alone.

## Audit D — persistent state and migrations

No production database was opened or modified during this audit.

### PostgreSQL

`migrations/` contains 36 lexically ordered, timestamped SQL migrations from `20260822000000_initial_domain.sql` through `20260910000000_sync_retention_handoff_proofs.sql`. `synveil-metadata` embeds the directory via SQLx and runs it through `MigrationRunner`; SQLx supplies migration locking and checksum/ledger handling in `_sqlx_migrations`. The wrapper reports applied, pending, unknown and failed versions and performs required-table probes. Failure is returned; it does not drop/reset a schema or fall back to SQLite.

| Order/domain | Migration files |
|---|---|
| Identity/auth/session | `20260822000000_initial_domain.sql`; `20260822000001_authentication_bootstrap.sql`; `20260822000002_browser_sessions.sql` |
| File metadata/upload/version/trash | `20260823000000_file_metadata_indexes.sql`; `20260824000000_upload_sessions.sql`; `20260824000001_file_version_history.sql`; `20260824000002_file_version_restore_operations.sql`; `20260824000003_trash_retention_metadata.sql` |
| Purge, object GC and sync | `20260825000000_metadata_purge_reference_accounting.sql`; `20260826000000_object_gc_planning_leases.sql`; `20260826000001_physical_object_gc_execution.sql`; `20260826000002_gc_worker_orchestration.sql`; `20260826000003_change_journal.sql`; `20260827000000_device_sync_checkpoints.sql`; `20260827000001_sync_rebaseline_bootstrap.sql`; `20260827000002_client_mutation_operations.sql`; `20260827000003_sync_conflicts.sql`; `20260828000000_device_credentials_enrollment.sql` |
| Backup/retention/restore/prune/schedule | `20260829000000_backup_domain_snapshot_manifest.sql`; `20260829000001_backup_content_retention.sql`; `20260829000002_backup_restore_plans.sql`; `20260829000003_backup_restore_execution.sql`; `20260829000004_backup_prune_plans.sql`; `20260829000005_backup_prune_execution.sql`; `20260829000006_backup_retention_expiry_planning.sql`; `20260829000007_backup_expiry_execution.sql`; `20260829000008_backup_maintenance_runs.sql`; `20260831000000_backup_read_api_indexes.sql`; `20260831000001_backup_operation_feed_indexes.sql`; `20260902000000_backup_scheduling_domain.sql`; `20260902000001_backup_schedule_occurrences.sql`; `20260902000002_backup_schedule_handoffs.sql`; `20260903000000_backup_misfire_policy.sql`; `20260903000001_backup_scheduled_maintenance_claims.sql` |
| Durable rebaseline and retention handoff | `20260908000000_rebaseline_durable_snapshots.sql`; `20260910000000_sync_retention_handoff_proofs.sql` |

The PG17 CI migration gate passed a fresh database applying all 36 migrations in run 39377173109. `migrations/README.md` says the migration set is forward-only and reviewed/immutable after release. Some migration services use explicit transactions and constraints to preserve durable facts. No supported downgrade/rollback set or prior-release-to-current upgrade fixture was identified. The migration README specifically documents a fail-closed condition for legacy conflict rows that lack typed source intent; that path is not silently reconstructed or dropped.

### Local/client state

| Store | Schema/version mechanism | Persistent data and evidence |
|---|---|---|
| Desktop Rust client SQLite | Eight SQLx migrations `0001_initial.sql` through `0008_first_sync_completion.sql`; `_sqlx_migrations`; `LOCAL_SCHEMA_VERSION=8`. Startup validates ordered success, checksums/known schema objects and columns; unknown/future/corrupt schema fails closed. Migration test module covers rollback and unknown schema behavior. | Server profiles, local replica/projection, sync checkpoints and pending operations/conflicts. OS SecretStore holds bearer secrets separately. |
| Android Room SQLite | `SynveilCacheDatabase` at schema version 4, with explicit migrations 1→2, 2→3 and 3→4. | Profile-scoped logical node/content metadata cache. Android has separate Preferences DataStore files for profiles, enrollment/credential metadata, sync settings/scheduler and recovery state; profile representation declares schema version 1. Android Keystore protects the AES-GCM key and bearer ciphertext is stored as a scoped envelope. |
| iOS primary SQLite | `MutationQueueSQLiteStore` uses `PRAGMA user_version`, current schema 9, explicit versioned forward migrations, `BEGIN IMMEDIATE`/commit and schema/integrity checks. Tests cover migration rollback and preservation. | Profile/owner/device/library scoped mutation queue, sync checkpoints/acks, rebaseline, node projections and transfer operations. Bearers remain in Keychain; file bytes stage in app files. |
| iOS photo-observation SQLite | Separate `PhotoObservationSQLiteSchema`, `PRAGMA user_version`, current schema 3 and transactional schema updates. | Opt-in photo observation/backup policy and local candidate/job metadata. |
| Browser | HTTP-only session cookie plus readable CSRF cookie; server session record lives in PostgreSQL. No browser SQL database or local secret vault was found. | Browser storage/version migration compatibility beyond server cookie/session contract is not established. |
| Server object/config files | Filesystem/ObjectStore content is outside SQL migrations; runtime config and secrets are separate deployment inputs. | PostgreSQL/object backup coordination and restore are operator responsibilities; app-level backup snapshots do not constitute a full host/database restore. |

### Migration and recovery risks

All database families are forward-oriented; no general downgrade guarantee was found. SQLx ledger/checksum validation and local fail-closed probes reduce unsafe startup but do not restore data after disk/database loss. SQLite tests exercise transaction rollback and preservation; they are not a substitute for cross-release production data snapshots. No end-to-end server DB plus object-store backup/restore rehearsal was recorded. Future schema additions must preserve durable queue identity, profile/owner/device scopes, conflict history, snapshot references and transfer recovery state.

## Audit E — compatibility and interface baseline

The matrix and contract invariants are in [P001_COMPATIBILITY_BASELINE.md](P001_COMPATIBILITY_BASELINE.md). At a high level: server API v1 ownership/authentication, browser cookie+CSRF semantics, separate device bearer principal, immutable logical file/version identity, Postgres metadata/ObjectStore bytes, durable local client state and cautious sync/conflict recovery are current contracts. Supported compatibility across different released server/client versions remains **UNKNOWN** because v0.2 is unreleased and no minimum supported-version matrix or live cross-version test was found.

## Audit F — existing security boundaries

| Boundary | Existing behavior found | Status / gap |
|---|---|---|
| Passwords and initial account | Argon2id password verifier; first-admin bootstrap is singleton and one-time. There is no general registration or discovered role-management UI. | `IMPLEMENTED_UNVERIFIED`; no general RBAC model. |
| Browser session | 256-bit random session credential; only SHA-256 digest persists; expiry/revocation record in Postgres; production session cookie is host-only, `HttpOnly`, `Secure`, `SameSite=Lax`; CSRF is session-bound HMAC double-submit plus provenance checks. | `IMPLEMENTED_UNVERIFIED`; behavior depends on configured TLS origin/proxy. |
| Device auth/enrollment | One-time high-entropy grant, digest-only server storage, short grant TTL (default 10 minutes, DB max 15), distinct DeviceCredential principal and owner-scoped revocation. Credential bearer auto-expiry/rotation was not found. | `IMPLEMENTED_UNVERIFIED`; QR pairing UI is `MISSING`. |
| Resource authorization | Owner/library/device-scoped routes and DB constraints; protected operations require an explicit auth class; clients cannot choose object paths. | `IMPLEMENTED_UNVERIFIED`; general multi-account RBAC and role assignment are `MISSING`. |
| TLS/network | Mobile production profiles require HTTPS; server API is placed behind a managed or operator edge; local/LAN modes use managed HTTPS/CA; external HTTPS proxy is operator-owned. Postgres is a private server input. | `PARTIAL`; external proxy/CDN redirect and network policy remain operator responsibilities; no Tailscale control integration. |
| Secret storage | Desktop Linux Secret Service/Windows Credential Manager, Android Keystore-encrypted bearer envelope, iOS Keychain. Unsupported desktop stores fail explicitly, with no disk fallback. Server credentials are runtime/deployment inputs. | `IMPLEMENTED_UNVERIFIED`; platform release acceptance absent. |
| Integrity | Upload/object paths verify expected length/hash; release candidate manifests record size/hash/source; backup snapshot manifests record logical content references. | `PARTIAL`; candidate hashes are not publisher signatures, and no Protected Areas integrity/approval workflow exists. |
| Logging/audit | Request/trace IDs, route/status/latency and sanitized error envelopes; code tests redaction of headers, bodies and token-bearing URIs. Domain records retain credential revocation, sync conflict and backup operation outcomes. | No generic persistent security audit/event log or approver attribution table/API was found (`MISSING`). Request tracing is not audit logging. |
| Privilege boundary | Current-user Windows Setup; Linux native package manager privilege; managed systemd units and dedicated service users; runtime configs/credentials separated from package-owned payload. | `IMPLEMENTED_UNVERIFIED`; full native host evidence incomplete. |
| Fail-closed behavior | Unknown auth, CSRF, migration schema, future client schema, invalid profile and missing trusted P036 inputs reject/stop rather than silently degrade. | `VERIFIED` only for narrow code/tests/runs; not a system-wide assurance. |

### Current security check result

The current base-commit Actions checks include a real `Dependency policy` failure on run [39885328513](https://github.com/nghianguyen150612/Synveil/actions/runs/39885328513/job/120192066214). The log identifies locked `rustls-webpki 0.103.15` through `rustls 0.23.46`/`reqwest 0.12.28` and advisories [RUSTSEC-2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381) and [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382). The reported fix range begins at `rustls-webpki >=0.103.16`. This is an unresolved security gate; P001 did not edit dependency files. Do not merge while this check remains failed.

No general change-approval, Protected Areas policy, passkey/FIDO2, audit-log management, access-token scopes/RBAC, network-mount, CLI push or SynveilNet/Tailscale client was found. These are recorded as future-scope gaps, not P001 implementation work.

## CI, source validation and scope control

Relevant repository workflows include `ci.yml`, `postgres-17.yml`, `release-artifact-candidate.yml`, `linux-packages.yml`, `appimage.yml`, `windows-installer.yml`, `windows-native-acceptance.yml`, `linux-clean-machine.yml`, `android.yml`, `ios-static-validation.yml`, `ios-build.yml`, `ios-simulator-tests.yml`, and `ios-rust-apple-build.yml`. CI workflows build candidate artifacts and execute focused platform tests; workflow success is not release approval.

At the 2026-10-23 capture, CI for exact baseline SHA `604499f…` was still running in several suites. The base commit `Dependency policy` check had failed on the RustSec advisories above. See [P001 manifest](../manifests/PROMPT001_MANIFEST.md) for the exact local audit-validation and requested test commands/results; see PR checks for the audit branch after PR creation.

`origin/develop/v0.3` was otherwise unchanged at preflight. The audit change list is constrained to these artifacts: this report, the compatibility baseline, the P001 manifest and `scripts/validate_p001_audit.py`. No runtime source, migrations, database or existing integration was changed.

## Remaining blockers and unknowns

1. v0.2.0 stable designation is not justified; P047/P048, signing, P036, native qualification, upgrade and stable-channel gates remain blocked.
2. Current dependency-policy scan flags two certificate-chain advisories; the required supported-version remediating build/scan is outside P001 scope.
3. v0.3 authoritative roadmap was not available in the workspace/supplied context.
4. No v0.2 supported client/server version compatibility or version-skew matrix was found.
5. No end-to-end restore of PostgreSQL plus object bytes, database downgrade, real host power-loss recovery, or supported v0.1.0-to-v0.2.0 upgrade was evidenced.
6. Android/iOS source and test infrastructure exist, but signed public artifacts and live compatible-server/device acceptance are not established.
7. GitHub hosted workflow artifacts are temporary; retain their current Action IDs/digests as evidence, but do not rely on them as distribution artifacts.

## P001 audit gate

P001 is complete when the scoped docs, validator and actual local/GitHub checks are recorded in [PROMPT001_MANIFEST.md](../manifests/PROMPT001_MANIFEST.md). This audit does not certify v0.2 as stable. The PR is intentionally left unmerged while any applicable required check/security gate fails.
