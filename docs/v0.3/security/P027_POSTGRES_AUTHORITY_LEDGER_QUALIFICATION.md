# P027 PostgreSQL authority ledger qualification

Status: provisional, isolated reference qualification. Production RBAC, role persistence, approval APIs, pairing activation and token issuance remain disabled. PostgreSQL reference evidence is not production security approval. The authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in this checkout; reconcile scope when it becomes available.

## Verified starting state

Repository: [Synveil](https://github.com/nghianguyen150612/Synveil). Base and fetched `develop/v0.3`: `c55ca38eb3fcfac030ad90e9863a5d16475e3fd0`, merge of [P026 PR #187](https://github.com/nghianguyen150612/Synveil/pull/187). P026 source `b772943fa5bc98f3f32bcfb7ff8db846e852e9ec` is an ancestor of that exact base. Remote P027 branch initially matched the base; no existing P027 PR or commits were found. The new clean checkout at `/workspace/Synveil` has one worktree, no unrelated changes, and uses `feat/v03-p027-authority-ledger-reference`. No reset, force-push or history rewrite is part of delivery.

## Production architecture and database audit

The source audit covers the P025 evaluator, P026 `rbac_v2`, their reference/SQLite tests, the PostgreSQL auth/device repositories, API router/auth/files/mutations/backups, metadata backup execution, and P003/P004 boundary records. The P026 55-row negative matrix and SQLite schema are retained unchanged.

| Boundary | Existing implementation and implication |
|---|---|
| Authentication | `crates/auth/src/lib.rs` and `crates/api/src/auth.rs` resolve browser sessions and distinct device credentials. PostgreSQL stores credential digests, status, owner/device identity and expiry/revocation. P027 stores fixture identity references only; it never issues or accepts real bearers. |
| Resource ownership | API file/backup handlers pass authenticated owner IDs to metadata backends; device mutation handlers bind authenticated device IDs. A session alone does not authorize arbitrary resource IDs. |
| Policy evaluation | P025/P026 accept independently trusted actor, ownership and grant inputs. Pure `Allowed` is a reference decision, not a durable execution capability. P027 reloads those inputs from its isolated ledger and calls `rbac_v2::qualify`. |
| Delegated authority | P026 verifies issuer/recipient continuity, exact scope, revision, expiry, explicit deny, bounded depth and ancestor state. P027 persists those records and immutable parent links; it does not activate delegation in the API. |
| Independent approval | P004-D06 remains pending. P027 persists synthetic exact-operation decisions with stable human identities and independently configured approvers. User context cannot suppress the stored requirement. |
| Transactional execution | P027 atomically consumes evidence, inserts a synthetic database operation, audit and outbox. It never performs a filesystem, network or OS effect. |
| Security audit | Existing host installation journals describe host operations. P027 database events provide transaction consistency, not signatures, independent retention, access policy or tamper evidence against database administrators. |
| OS privilege | P003 zone Z-06 and native installer/host authorization remain distinct from API identity and roles. An application role grants no root/administrator authority. P027 has no OS execution adapter. |

Existing PostgreSQL transactions are already substantive: `metadata/src/auth.rs::create_first_admin` locks singleton `bootstrap_state` with `FOR UPDATE` before creating user, credential and closure marker. `metadata/src/device_credentials.rs` documents owner → device → grant/credential lock ordering; grant exchange reloads current device/grant state and evaluates expiry after waiting. Device revocation shares these locks. `metadata/src/backup.rs` uses transactions and owner/set/plan/snapshot locks for restore, prune and expiry; ordered snapshot locks and revalidation precede metadata changes. These guarantees apply to those repository operations and do not establish independently authenticated authorization for every public repository primitive or scheduled maintenance adapter.

API `router.rs` applies browser/inbound authentication and CSRF layers. `backups.rs` passes `auth.owner_user_id()` to plan creation/execution/status and maintenance operations. `mutations.rs` combines authenticated owner/device with library scope. Ownership and optimistic concurrency are also enforced by metadata services. Backup deletion, expiry, object GC and installed maintenance cross service, repository and external ObjectStore boundaries. This inspection does not close the exhaustive maintenance call-graph audit inherited from P025/P026.

P003 trust zones Z-03/Z-04/Z-05/Z-06 remain separate. P004-D05 owner-to-role mapping, D06 approval/audit and D09 CLI/token scope remain `RECOMMENDED_PENDING_APPROVAL`; fixture viewer/editor names are not approved production roles. No production migrations, live middleware, clients, credentials, enrollment, mobile vaults, queues or checkpoints change.

## Isolated persisted schema and trusted inputs

`crates/auth/tests/support/p027.sql` is compiled only by integration tests. Tests require both `SYNVEIL_P027_DATABASE_URL` and `SYNVEIL_P027_DISPOSABLE=1`. Each test creates a generated `p027_<UUID>` schema; the pool sets its search path to that schema and `pg_catalog`. Teardown validates the generated identifier and drops only that schema. Production migrations are never applied by this harness. The separate historical regression runner uses fresh databases in the explicitly disposable instance.

| Store | Qualified representation |
|---|---|
| `policy` | Supported version, monotonic policy/role revisions, trusted approval requirement, delegation mode and authority fence epoch. |
| `actors`, `sessions` | Fixture authenticated identity, stable human identity, optional exact device, actor/session revocation. Session IDs are references, never credential material. |
| `ownership` | Exact resource, owner, installation/host, resource type/action/device, authenticated owner actor, independently designated approver and legacy result. |
| `grants` | Immutable unique grant ID, issuer, recipient, parent FK, role/revisions, exact scope, maximum action, validity and explicit deny. All update/delete is rejected. |
| `revocations` | Immutable unique grant tombstones with reason/time; no grant replacement, tombstone deletion or restoration. |
| `approvals` | Unique decision/request IDs, exact operation/requester/approver/scope/revision/validity, irreversible withdrawal and consumption. Decision fields cannot change. |
| `operations` | Immutable unique operation identity, canonical reference payload, authenticated actor, grant, approval, revision and synthetic outcome. |
| `audit`, `outbox` | Append-only application events and one synthetic outbox record per operation, written in the same transaction. No delivery worker is enabled. |

FKs reject nonexistent parents and actors. Self-parent checks and a recursive insert trigger reject cycles, including mutually referencing rows inserted in one statement; immutable IDs and parent links prevent later graph replacement. P026 also rejects cyclic/missing graphs independently. Duplicate IDs fail; revoked IDs cannot be restored with updates, deletion or reinsertion. A new legitimate grant requires a new ID and current issuer authority. Scope equality, issuer provenance, role vocabulary, current revisions, deny precedence and delegation attenuation are evaluated from persisted rows by P026. The schema itself is not an authenticated issuance service. Trusted fixture setup can intentionally seed invalid-but-representable records for negative tests; those records never become positive production evidence.

## Transaction isolation and lock order

Normal execution, revocation and delegation begin `SERIALIZABLE`. First update the singleton policy epoch. This globally serializes authority operations and makes a stale snapshot fail with `40001`, even if the conflicting authority mutation merely inserts a tombstone. This deliberately conservative reference strategy is not a scalable production lock design.

Execution order is policy fence → exact session row → actor rows ordered by ID → exact ownership row → all grant rows ordered by ID (including ancestor/deny records) → approval rows ordered by ID → conditional consumption → operation/audit/outbox insert → commit. Immutable operation status may be read after session resolution without a row lock. Revocation uses the same fence then ordered grant rows and a tombstone/audit insertion. Delegation uses the fence and the same ordered persisted policy snapshot before issuing a fresh immutable child. Policy/ownership/revocation contenders in ordering tests acquire the fence before mutation. Fixture setup outside active contenders can seed deliberately invalid authority; it is not a supported concurrent writer.

Every participating authority writer must take the fence. Production writers, privileged SQL clients and future per-resource lock strategies are outside this qualification. The global fence prevents adapter lock cycles; a dedicated probe deliberately reverses actor locks to observe a real `40P01` deadlock. Future cross-resource transactions, foreign-key locks, external writers or new lock orders require new qualification.

Retries are limited to three total attempts, only for SQLSTATE `40001` and `40P01`. The entire transaction restarts with the same operation identity, fresh persisted authority and fresh database time. Other errors propagate, including duplicate IDs, connection loss and ambiguous commit responses. There is no unconditional loop and no approval consumed outside its transaction. Expiry uses `clock_timestamp()` after authority locks, rather than transaction-start time or a caller-supplied clock. The expiry-after-wait probe uses an explicit READ COMMITTED contender to isolate wall-clock handling; normal adapter operations and serialization-failure probes use SERIALIZABLE.

## Revocation, approval and idempotency semantics

After revocation commits, a new operation cannot use the grant or any descendant. Child rows need no rewrite: loading the current ancestor tombstone makes P026 deny. Issuance racing revocation may commit before it, or reload and reject afterward. A revoked actor cannot create a new child. Policy revision, role revision or ownership changes invalidate old authority. Ordered tests prove database blocking with `pg_blocking_pids` and commit the blocker before checking the contender; a prior valid execution is distinguished from a post-revocation bypass.

Approval requirements and approver designation come from stored authority. The request's `approval_required=false` is not permission to bypass approval. Evidence binds request ID, trusted requester, distinct principal and distinct human approver, operation, owner, resource, host, action, optional device, policy revision, issue/expiry, withdrawal and consumption. The adapter evaluates each candidate separately before conditionally consuming that exact candidate. The successful operation/approval relation is bidirectional, unique and checked by a deferred FK at commit. Self-approval, same-human sessions, wrong operation/scope/revision, expired/withdrawn/consumed decisions fail.

Operation identity is stable across retries. The reference payload length-prefixes session, grant and complete request context; the context uses Rust's diagnostic encoding and is deliberately not a production wire-format promise. Reusing an identity with a different payload returns `IdentityConflict`. The same authenticated requester and identical payload can recover a committed outcome without consuming evidence or executing again, even after grant revocation. Recovery reports metadata only. A new operation still reloads current authority; a revoked actor/session is rejected even for status recovery. A lost response must lead to status lookup, never resetting approval state.

`effective_allow = legacy_allow AND proposed_policy_allow` is preserved. Legacy denial always denies, including with an otherwise valid grant and approval. No owner is automatically assigned a new role.

## Audit, outbox and crash boundaries

Audit events cover seeded trusted grant/approval decisions, delegation, denial, approval consumption, revocation, synthetic execution and ambiguous-outcome recovery. Successful execution assertions join operation, consumed approval, outbox and both audit events on the same PostgreSQL transaction ID. A denial has no operation/outbox effect. Rollback, backend termination and process exit remove uncommitted consumption, operation and success events together. Sequence gaps after rollback are permitted and do not indicate success.

`pg_terminate_backend` is exercised on a separate live connection before commit. A real child test executable calls `std::process::exit(27)` with an open transaction; it does not gracefully drop or roll back Rust objects. The parent waits for the exit code and then acquires the authority fence to await database rollback. Committed grants survive that child failure. Reconnect verifies committed revocation/consumption and operation recovery. A two-phase probe additionally permits the runner to prepare a committed operation/tombstone, restart the actual disposable PostgreSQL container, then verify persistence from a new test process.

An ordinary database-container restart is not a power-loss, kernel-crash, fsync-corruption or hostile-database test. Transactional audit is not tamper-evident audit. The outbox contains synthetic database effects only; there is no claim of atomic external filesystem mutation, network delivery, OS privilege or exactly-once external execution.

## Qualification inventory

| Tests | Evidence |
|---|---|
| `execution_consumption_idempotency_and_recovery` | Durable success, reconnect, lost response, recovery during revocation and cross-resource identity conflict. |
| `concurrent_consumers_and_duplicate_operations` | One execution, one recovery, no second approval consumption, different-operation replay rejection. |
| `immutable_ids_parent_integrity_and_history` | Duplicate IDs, invalid/self parent, grant replacement, audit modification, policy rollback and tombstone restoration reject. |
| `ordered_authorization_revocation_and_stale_retry` | Proven lock wait and real serialization failure for ancestor revocation, policy/role revision, ownership and actor revocation; reverse commit order and new-operation denial. |
| `delegation_issuance_revocation_and_duplicate_races` | Issuance versus parent revoke, concurrent issuance, unique-ID collision and revoked issuer rejection. |
| `approval_withdrawal_expiry_and_lock_time_expiry`, `expiry_is_reloaded_after_a_proven_lock_wait` | Withdrawal monotonicity, expired grant/approval, database-clock expiry after a proven fence wait. |
| `adversarial_persisted_authority_matrix` | 15 persisted-authority negative rows and 9 request substitutions: forged principal/owner/issuer/role; stale role/policy/approval; host/device/resource/action replay; unsupported vocabulary/version; expired ancestry; wrong/same-session approver; legacy denial; OS-root request. |
| `same_human_self_approval_and_deny_precedence`, `exact_device_binding_positive_and_replay` | Valid approver designation still fails same-human approval; conflicting deny wins; valid device execution followed by cross-device rejection. |
| `connection_termination_rolls_back_effect_approval_and_audit`, `subprocess_exit_without_destructors_recovery` | Backend termination, real process exit, uncommitted grant/audit/effect rollback, committed authority recovery. |
| `real_deadlock_sqlstate_and_bounded_retry_classification` | Actual `40P01`, retry classifier, non-retryable error, PostgreSQL 17 assertion. |
| `durable_reconnect_and_database_restart_probe` | Reconnect in normal suite; external prepare/restart/verify protocol for actual database restart. |
| Unchanged P025/P026 | 3 P025 tests, 5 P026 reference tests including 55 negative rows, 5 SQLite durability/race/crash tests. Cyclic delegation, missing parent, amplification and stale retry semantics remain covered across these and PostgreSQL constraints/order tests. |

No test fabricates a passing PostgreSQL result. Live tests are explicitly ignored by default under repository conventions; the focused workflow selects `--ignored`, checks the exact count and fails without a reachable disposable database. Windows explicitly compiles this harness and runs contracts/SQLite/pairing/installer/sync tests. Repository service-container infrastructure is Linux-only; actual Windows-hosted PostgreSQL races are unavailable and are not inferred from compilation.

## Validation and remaining gates

Actual command results, counts, commit identifiers and hosted status are in [the P027 manifest](../manifests/PROMPT027_MANIFEST.md) and the Draft PR. Queued, failed, cancelled, skipped and unavailable checks are not passes. Global Rust CI and existing PostgreSQL workflows remain required independent evidence.

Open: P004-D05/D06/D09; D12/D13 pairing trust and lifecycle; all remaining P004 decisions; exhaustive P025/P026 maintenance call-graph audit; authenticated production authority provenance/issuance; production approval/audit/privacy/retention; external-effect/outbox integration; independent security review; P028 native Windows acceptance; P036 signing/publisher qualification; physical Android/iOS pairing/recovery; real LAN/Tailscale; authoritative roadmap reconciliation. Historical Windows installer missing-LICENSE and secret-marker investigations remain separate unresolved root-cause gates. Recent passing installer resilience does not close them. This work starts no P028 implementation and leaves its PR unmerged.
