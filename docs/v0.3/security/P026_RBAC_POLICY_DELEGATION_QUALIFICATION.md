# P026 RBAC policy, permission and delegation qualification

Status: provisional reference qualification, integration-only; **not production ready**.
The P001–P060 authoritative roadmap remains unavailable (searched repository docs); reconcile scope when recovered. P025's integration-only security exception is inherited, not discharged. P004-D05/D06/D09 remain RECOMMENDED_PENDING_APPROVAL. P003 authentication, application permission, approval and native OS boundaries remain separate.

## Recovery and source

Base `14c2dbf51464d6893c1f19a8c1f8e041be5b338e` is P025 merge PR #186; source `54918f457784edb29b52dcbdac79c3539e33a3d8` is an ancestor. P026 remote branch initially had no implementation; no existing P026 PR was found. Clean initial checkout `/workspace/Synveil` on `work` at `604499f` was preserved. Separate worktree `/workspace/Synveil-p026` uses `feat/v03-p026-rbac-policy-delegation-qualification`. No resets, history rewrites, production migrations or credential reissuance.

## Production authority audit

Reviewed `api/src/router.rs`, authentication middleware and handler families below, `auth/src/service.rs`, `auth/src/device_credentials.rs`, metadata ownership and revocation queries, P003 trust boundaries and P004 decision packet. The route inventory below enumerates current router handler entry points. This is a source audit, not proof that every internal alternate caller or production race is safe.

| Entry family | Actual authority enforcement | Reliance / mutation or deletion |
|---|---|---|
| Browser login/session/logout/CSRF | AuthenticationService verifies password, digest, session expiry/revocation and active user. `require_authentication` rejects any Authorization header. `require_csrf_for_mutations` validates browser provenance/proof. Bootstrap boundary and repository singleton govern first admin. | Middleware authenticates, does not authorize arbitrary resource IDs. Logout revokes session; password replacement and cleanup are trusted service calls. Login/CSRF are not independent approval. |
| Device enrollment/exchange/revoke | Browser-only enrollment/revoke routes; DeviceAuthenticationService uses owner/device scoped repository. `lock_active_owner`, `lock_device`, `load_grant` use row locks. Digest exchange checks interval/consumption; credential lookup joins matching owner/device and active owner. | `create_grant`, `revoke_credential`, `revoke_all_credentials`, `revoke_device` trust authenticated owner argument. Revoking a device also revokes credentials and unconsumed grants. Exchange is public but requires enrollment secret; not browser authority. |
| Mixed inbound authentication | `require_inbound_authentication` rejects mixed cookie/bearer and never falls back from failed bearer; no browser AuthContext installed for device. | Device mutations skip browser CSRF only after successful device authentication. This does not make the device a browser administrator. |
| Library catalog, children, node metadata | Handlers derive owner from principal; FileApplicationService calls owner-scoped repository SQL joins. | `owner_user_id()` extracts verified scope; it does not itself validate resource ownership. Catalog creation sets authenticated owner. |
| Browser file mutations and version restore | `create_directory`, `update_node` (rename/move), `delete_node` (trash), `restore_node`, `restore_version` require browser + CSRF and use owner-scoped services. | Repository `create_directory_owned`, `rename_node_owned`, `move_node_owned`, `delete_node_owned`, `restore_node_owned`, `restore_file_version_owned` perform ownership/state checks. No role bypass. |
| Downloads/version metadata/history | ContentReadApplicationService resolves owner-scoped logical node/version before ObjectStore key access; history browser-only; inbound metadata/content mixed. | ObjectStore keys and streams have no user auth; rely on checked service caller. Range/ETag is not authority. |
| Upload create/get/append/complete/abort | Handlers pass authenticated owner to UploadApplicationService; repository session/library/node checks bind owner, size/hash/lease/state. | Transfer backend relies on upstream authenticated owner; completion publishes content and abort mutates staging. Device access here is existing owner-scoped behavior, not new device administration. |
| Device sync feed/checkpoint/ack; rebaseline start/pages/complete | Handlers call `require_device`, derive owner and pass library/device IDs to metadata sync scope checks. | `require_device` rejects bearer-device mismatch; browsers pass and still require repository owner/device/library checks. Ack/completion change checkpoints. |
| Durable rebaseline snapshot create/descriptor/pages/handoff | Owner argument checked in metadata snapshot service; handoff takes device from verified device principal. | Browser snapshot reads/creation are existing semantics; handoff is device-bound. Snapshot IDs/cursors are not owner authority. |
| Device mutation submission | `submit` calls `require_device`; repository `load_client_mutation_scope`/`validate_client_mutation_scope` require device and library owned by same authenticated owner, active status, revision and journal checks; write path locks device/library. | Supports create/rename/move/delete/restore effects; idempotency does not bypass scope checks (P005 regression retained). |
| Conflicts list/detail/resolve | Browser authentication; metadata conflict scope and signed cursor binding to owner/device/library. | Resolve mutates files/versions through scoped transaction; browser device parameters rely on metadata scope, not `require_device`. |
| Backups sets/snapshots/nodes/operations/retention/maintenance/restore/prune | Browser+CSRF for writes, authenticated owner passed to BackupApplicationService; metadata backup joins include owner and locked plan/target rows. | Create set, configure retention, start/advance maintenance, create/execute restore and prune plans mutate state. Pruning and maintenance can remove references; byte GC remains separate. Plan IDs alone are not authority. |
| Trusted repository primitives | `find_owned_library`, `find_owned_node`, `find_owned_node_in_library`, `find_owned_file_version`, `list_owned_*`, `load_owned_*` filter owner/resource. | Public `find_user/device/library/node/object/file_version`, `insert_*`, `update_node` are lower-level primitives, **not authorization boundaries**; callers must enforce ownership and active state. Never expose directly as role APIs. |
| Internal workers and migration/admin composition | `synveil-worker`, `synveil-scheduled-maintenance-once`, `synveil-server-migrate` use operator DB credentials and configured runtime boundaries, not browser roles. Retention/purge/physical_gc/gc_worker/backup/scheduler modules can mutate/delete metadata or bytes. | These trusted internal services rely on upstream plan selection, candidate/reference checks, DB privilege and OS service isolation. Exhaustive call-graph and concurrent privilege review of every maintenance adapter remains inherited debt. |
| Health/bootstrap status | Public health/readiness and bootstrap status reveal bounded deployment state; bootstrap mutation has separate boundary. | No role inferred from local network or health readiness. Host helper/installer effects require independent native authority. |

No production call to the reference evaluator was added. Existing authentication reads and later effects are not claimed to share a global revocation fence. In particular, this audit does not promote legacy browser privilege or internal repository access to an approved RBAC design.

## Version 2 reference contracts

`authorization_reference::rbac_v2` extends P025, reusing its authentication-class/audience/device validation, legacy intersection and OS privilege separation. Version 1 helpers remain historical P025 fixtures, including their trust assumptions; they are not hardened production interfaces. Version 2 introduces typed policy/role revisions, fixture role identity, resource type/ID, owner/installation, action and exact optional device scope. Only FixtureViewer(Read) and FixtureEditor(Read/Write/Delete) exist; neither defines production Owner/Editor/Viewer semantics. Unknown values fail closed; no wildcard evaluation, host inheritance, scope hierarchy or role union exists. Exact scopes are the only accepted scopes, so a broader assignment cannot override a restrictive device/resource binding. A matching denial wins regardless of allow order, including denials against an intermediate grantor.

Policy and all actor/ownership/grant records are trusted fixture-store inputs, never HTTP claims. Request grant IDs and role names are not authority. Owner identity is resolved separately to an authenticated browser actor; a string equal to the owner's display name is insufficient. Stable human identity is independent of session ID. Production provenance would require independently authenticated issuance, immutable grant IDs, verified grantor capability and transactional audit; the public Rust fixture structures do not establish that provenance.

Each record binds recipient, authenticated grantor, original owner, installation, exact resource/action/device, role and policy revisions, issue/expiry, tombstone and optional parent. Maximum permissions are deliberately one exact action. A child must preserve that scope/action and fit the parent's lifetime; each ancestor is rechecked for authority, denial, revocation and revision. One owner-authorized delegation is supported by default; further delegation requires trusted recursive policy and bounded depth. Missing parents, cycles, duplicate IDs, scope expansion, stale authority and revoked grantor actors deny. In-memory records alone cannot prevent replacement by stale copies; the durable harness provides immutable IDs/tombstones. They are not production persistence.

## Approval separation

Approval requirements come from trusted `Policy.approval_actions`; request `Context.approval_required=false` cannot bypass them. Fixtures resolve exact authorized browser approvers and stable actors independently. Approver must differ from requester both in principal and stable human identity. Evidence binds operation identity, requester, scope, revision, issue/expiry and withdrawal/consumption state. Duplicate evidence IDs fail closed. No production signatures, bearer approval tokens, approval endpoints or issuance exist.

A pure Allowed result is not execution authority. The SQLite fixture acquires an explicit write transaction, reloads current grant revisions/tombstones and approval state, re-evaluates policy, consumes evidence conditionally and records the synthetic effect/audit before commit. Only fixture code supplies positive evidence. Audit records are deterministic synthetic events, not a production tamper-evident ledger.

## Durable state and ordering

All reference schema is in `crates/auth/tests/support/p026.sql`. The harness persists revisions, tombstones, approval consumption/withdrawal and audit; the actor/ownership/delegation graph is a fixed trusted test fixture reloaded for each decision, not a durable role store. Tests use isolated temporary SQLite files, WAL, independent pool connections and `BEGIN IMMEDIATE` writes. Duplicate insertion and revoked-ID replacement fail; no restore/delete of tombstones. Parent tombstones propagate through evaluation without requiring child rewrites. Tests observe precommit state from another connection, explicit rollback, committed state, connection loss before commit, subprocess termination without transaction destructors, close/reopen recovery, lost response after consumption, grant creation versus revocation and approval execution races. A pre-revocation transaction may finish first; all subsequent execution reloads authority and denies. The synthetic effect is an audit insertion; no bearer credential is minted.

This does **not** prove PostgreSQL transaction safety, power-loss/OS-crash/fsync durability, network exactly-once delivery, or arbitrary application-effect atomicity. Production PostgreSQL requires SERIALIZABLE transactions with bounded retry of serialization/deadlock errors; stable lock ordering across policy revision, owner/device authority, all ancestor grants, exact resource and approval rows using `SELECT ... FOR UPDATE`; revocation must take the same locks. Unique immutable grant/evidence/operation IDs and irreversible tombstones are necessary. Authorization, conditional evidence consumption, idempotent effect or transactional outbox and audit must commit together. Retry after lost response must query operation status, never restore evidence. Effects outside PostgreSQL require separately qualified idempotent execution/outbox and recovery. Production locking tests with independent PostgreSQL connections remain open.

## Compatibility and negative inventory

| Legacy | Proposed | Effective |
|---|---|---|
| allow | allow | allow |
| allow | deny/unsupported/stale/approval-needed | non-allow |
| deny | any | deny |

Current browser sessions/CSRF, device credentials/enrollment/revocation, API v1, Android/iOS profiles and vaults, queues/checkpoints and P020–P024 pairing contracts are unchanged. No new table in product migrations, role migration, middleware replacement, owner reassignment, network listener or OS service.

`p026_reference` has 5 test functions: 29 exact-scope negative rows, 13 delegation negative rows, 13 approval negative rows, plus positive operations, recursive policy/depth, device binding, legacy denial and OS separation assertions. Specific Decision values are asserted for every negative row. Covered: unauthenticated/unknown identity; browser/device confusion; forged owner/role/grantor/delegation; cross-owner/device/host/resource; unknown action/resource/version; stale policy/role/assignment; expiry/revocation; deny precedence; expansion/invalid grantor; recursion/cycle/depth; revoked parent/actor; self-approval including same-human sessions; expired/withdrawn/consumed/wrong-scope evidence; host/operation replay; OS-root denial. `p026_transactions` has 5 test functions for independent-connection durability and races. P025's 3 historical tests are retained without relabeling their weaker negative assertions as full qualification.

## Validation and open gates

Validation results and commit/run evidence are recorded in the P026 manifest. Incomplete or cancelled jobs are not passes. Focused workflow checks out exact PR head on Ubuntu/Windows; global Rust CI remains unchanged. Existing P015 PostgreSQL workflow retains real owner/device isolation coverage. Local ignored PostgreSQL tests require a disposable PostgreSQL 17 instance and cannot be counted as passed without execution.

Inherited P025 debt remains open: exhaustive internal/maintenance call-graph audit, production authority provenance verification, production independent approval lifecycle, durable PostgreSQL revocation/concurrency and adversarial integration matrix. P026 qualifies fixture semantics only. Production activation requires independently approved taxonomy and P004-D05/D06/D09, authenticated authority storage, durable audit/outbox/recovery, migration/rollback plan, security review and exact-commit hosted/native evidence.

Independent release blockers: P028 requires real interactive GUI, genuine Windows logon and installed-component IPC evidence (unavailable). Windows installer resilience run `41627469609` failed because installed tree lacked LICENSE. Historical run `41437684142` reported `INSTALLER_VERIFY_FAILURE: secret-like marker in output`; these are distinct failures. No installer checks were weakened or suppressed. P036 publisher signing, physical mobile and real network qualification remain separate gates. Production RBAC, role persistence, approval endpoints and token issuance remain disabled.

## Router entry inventory at the audited base

Source: `crates/api/src/router.rs`; middleware names are included to distinguish authentication from handlers.

- `auth`: `bootstrap_boundary`, `bootstrap_status`, `create_first_admin`, `current_session`, `issue_csrf`, `login`, `logout`, `logout_boundary`, `require_authentication`, `require_csrf_for_mutations`, `require_inbound_authentication`.
- `backups`: `advance_backup_maintenance_run`, `configure_backup_retention_policy`, `create_backup_maintenance_run`, `create_backup_set`, `create_prune_plan`, `create_restore_plan`, `execute_prune_plan`, `execute_restore_plan`, `get_backup_maintenance_run`, `get_backup_operation`, `get_backup_retention_policy`, `get_backup_set`, `get_backup_snapshot`, `get_prune_execution`, `get_prune_plan`, `get_restore_execution`, `get_restore_plan`, `list_backup_maintenance_runs`, `list_backup_operations`, `list_backup_sets`, `list_backup_snapshot_nodes`, `list_backup_snapshots`.
- `conflicts`: `detail`, `list`, `resolve`.
- `device_auth`: `create_grant`, `exchange`, `revoke_all_credentials`, `revoke_credential`.
- `downloads`: `current_content`, `historical_content`.
- `files`: `create_directory`, `create_library`, `delete_node`, `get_node`, `list_children`, `list_libraries`, `restore_node`, `update_node`.
- `health`: `live`, `ready`, `system_health`.
- `mutations`: `submit`.
- `rebaseline`: `complete`, `get_nodes`, `start`.
- `rebaseline_snapshot`: `complete_handoff`, `create_snapshot`, `get_descriptor`, `read_entries`.
- `sync`: `acknowledge`, `get_changes`, `get_checkpoint`.
- `uploads`: `abort_upload`, `append_upload_chunk`, `complete_upload`, `create_upload_session`, `get_upload_session`.
- `versions`: `get_version`, `list_versions`, `restore_version`.
