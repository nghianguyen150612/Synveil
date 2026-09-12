# Database migrations

This directory is reserved for reviewed, ordered, forward-only PostgreSQL
migrations. `synveil-metadata` loads this directory through SQLx at runtime and
runs it through its one-shot `MigrationRunner`; SQLx owns the PostgreSQL
migration lock, checksum validation, and `_sqlx_migrations` bookkeeping.

The initial canonical domain migration creates only `users`, `devices`,
`libraries`, `nodes`, `objects`, and `file_versions`. Authentication sessions,
credentials, journals, outbox/jobs, sharing, and most replica/lifecycle work
remain later migrations. The authentication migration adds the documented
instance administrator flag, `user_credentials`, and a singleton persistent
`bootstrap_state`. The browser-session migration adds verifier-only `sessions`
rows with persistent expiry and revocation. The upload-session migration adds
the first verified `object_replicas` table and persisted resumable
`upload_sessions`; it does not add sync tables or HTTP transport. Product
migrations now add the bounded immutable file-version history index; they do
not mutate or delete historical `file_versions` rows. The restore-operation
migration adds only the owner-scoped persisted identity and committed outcome
needed to replay a successful historical-version restore without creating a
duplicate `FileVersion`; it does not add purge, retention, sync, or backup
state. The Trash-retention migration adds the single canonical `nodes.trashed_at`
timestamp and a partial `(trashed_at, id)` candidate-scan index; it does not
delete metadata, references, replicas, or object bytes. Product migrations must
be reviewed and immutable after release. A migration failure is
surfaced to the caller; the runner never drops schemas, resets a database, or
silently falls back to SQLite.
The metadata-purge migration adds the minimal completed-purge replay identity,
metadata-only object GC-candidate table, the deferrable self-parent constraint,
the Object-reference index, and the upload-parent reference index needed to
remove one node's immutable FileVersion history and evaluate survivors
set-wise in one transaction. The following GC-planning migration adds candidate
state, opaque lease/generation timestamps, lifecycle constraints, and bounded
claim indexes. The physical-GC execution migration then adds the minimal
`objects.lifecycle_state` fence, active hold registry, durable operation/action
records, and database triggers that reject new reference or hold acquisition
once an Object is `GC_DELETING`. The worker-orchestration migration adds only
bounded retry scheduling (`attempt_count`, `next_attempt_at`), recovery indexes,
and an explicit `NEEDS_ATTENTION` operation state; it contains no path, storage
credential, external queue, or deletion behavior. The runtime deletes bytes
only after these forward-only schema records and a final PostgreSQL
revalidation; it records each replica outcome before ObjectReplica/Object
cleanup. The change-journal migration adds per-library `journal_epoch`, gap-safe
transactional `sync_head`, retention watermark metadata, the typed
owner/library-scoped `change_journal` table, ordering indexes, and a database
append-only trigger. Purge tombstones have no foreign key to deleted Nodes or
FileVersions. The device-checkpoint migration adds the durable
owner/device/library scope for one-way synchronization. It requires an
existing registered Device and owner-owned Library through composite foreign
keys, stores only the current journal epoch and acknowledged sequence plus
server timestamps, and gives each device independent progress per library. It
does not store journal payloads or physical storage identities. Checkpoint
progress is advanced by the metadata service only after a bounded signed
delivery token is verified. Snapshot/rebaseline progress is implemented by the
Prompt 33 forward-only migration. It adds a
monotonic `rebaseline_generation` fence to each checkpoint, one scoped
`sync_bootstraps` state machine (`OPEN`, `COMPLETED`, `ABORTED`, `EXPIRED`),
and immutable `sync_bootstrap_nodes` logical manifest rows ordered by Node ID.
The session binds owner, ACTIVE registered device, owned library, generation,
journal epoch, resume sequence, manifest terminal/count, and PostgreSQL/server
timestamps. Manifest rows include current logical Node state and safe current
content length/hash only; they intentionally have no foreign key back to
mutable Nodes/FileVersions and contain no Object/ObjectReplica ID, key, path,
staging handle, backend locator, credential, GC state, or bytes. Cleanup
cascades only from a retired bootstrap to its copied manifest. The client
mutation-operation migration adds the owner/device/library-scoped mutation ID
primary key, versioned SHA-256 fingerprint, closed typed kind and base
epoch/sequence, terminal `APPLIED`/`CONFLICT` result state, safe logical result
or conflict projection, and server timestamps. The operation row contains no
raw JSON patch, file bytes, object/replica identity, path, storage locator, or
credential. The metadata service inserts and terminalizes it atomically with
the canonical Node change and exactly one journal event; transient failures
roll back the operation and journal fact. A purged resource can produce only a
`RESOURCE_PURGED` conflict from the retained journal tombstone. Automatic
conflict resolution, backup, sharing, and public physical-GC controls remain
outside these migrations.

The Prompt 35 forward-only migration adds `sync_conflicts` and
`sync_conflict_resolutions`, plus the typed `conflict_id` linkage on the
original mutation operation. Closed typed columns preserve the original intent
and historical logical observation without raw request JSON or physical
storage metadata. Deferred scoped foreign keys enforce the bidirectional
terminal-operation/conflict invariant. Partial immutable keyset indexes serve
bounded OPEN listing. Database triggers allow only `OPEN` to `RESOLVED` or
`DISMISSED` lifecycle movement and one `IN_PROGRESS` resolution to a terminal
result; original evidence and completed decisions cannot be rewritten. Conflict
rows deliberately have no Node foreign key, so rebaseline and resource purge
cannot erase their audit evidence. A successful manual apply links the normal
canonical journal event; accepting server state and stale apply attempts have
no journal linkage. There is no conflict-retention deletion or automatic
resolution workflow.

Because Prompts 31–34 are an unreleased, intentionally uncommitted development
stack, the migration fails closed if it encounters pre-existing terminal
Prompt 34 `CONFLICT` operations that lack the typed original intent needed to
construct immutable Prompt 35 evidence. Fresh migration from zero is supported
and validated. A future upgrade fixture must define an explicit typed backfill
before this preview-only guard can be relaxed; the migrator never fabricates
evidence from raw JSON or drops those operations.

## Prompt 37 server and desktop migrations

`20260828000000_device_credentials_enrollment.sql` adds only canonical
`device_credentials` and `device_enrollment_grants`; it reuses `devices` and
its owner-pair uniqueness from Prompt 32. UUIDv7 credential/grant IDs are
non-secret. Both tables store only 32-byte, purpose/version-domain-separated
SHA-256 verifiers, never raw bearer or enrollment strings. Composite Device/
owner foreign keys, digest length/uniqueness, version, and timestamp checks
fail closed. Grants have a default service TTL of 10 minutes and a database
maximum of 15 minutes; consumed rows link to exactly one credential with the
same owner/Device. Credential audit rows are retained.

The auth service atomically creates a canonical PENDING Device with its grant,
or scopes a grant to an owned PENDING/ACTIVE Device. Exchange locks owner,
Device, and grant, checks expiry/revocation/consumption, activates a PENDING
Device, inserts the credential digest, and consumes the grant in one
transaction. Production reads a fresh expiry clock after acquiring all three
locks; a queued request cannot use its pre-lock timestamp to accept an expired
grant. No consumed grant reissues a secret after lost HTTP response.
Explicit browser-owner revoke-all invalidates all credentials and unused
grants without revoking the Device, allowing a fresh grant for the same Device.
Single-credential revoke and Device lifecycle revoke are also supported;
authentication reads current owner/Device/credential state per request.
Automatic credential rotation/expiry and cleanup of audit rows are not added.

Desktop SQLite is a separate local-state authority, not a server database
fallback. Its forward-only
[`0002_server_profiles.sql`](../crates/client-sync/migrations/0002_server_profiles.sql)
adds non-secret server profiles, enrollment identities/timestamps, cleanup
intents, and explicit replica/profile binding. It preserves initial migration
`0001_initial.sql`. Immutable identity triggers prevent origin/owner/Device/
profile rebinding; legacy unbound replicas remain unbound rather than inferring
a server from Library ID. Bearers stay exclusively in the existing platform
SecretStore, keyed by opaque profile/credential identity. There are no password,
bearer, enrollment-token, cookie, or credential-export columns.

### Migration Prompt 37 — tiếng Việt

`20260828000000_device_credentials_enrollment.sql` chỉ thêm bảng chuẩn
`device_credentials` và `device_enrollment_grants`, tái dùng `devices` cùng
owner-pair unique của Prompt 32. Credential/grant ID là UUIDv7 không bí mật;
server chỉ lưu SHA-256 verifier 32 byte với purpose/version domain riêng,
không lưu raw bearer/enrollment string. Composite Device/owner FK cùng check
digest/version/timestamp fail closed. TTL grant mặc định 10 phút, DB giới hạn
15 phút; row consumed link đúng một credential cùng owner/Device. Giữ row
credential để audit.

Tạo Device PENDING mới cùng grant, hoặc grant cho Device PENDING/ACTIVE thuộc
owner, dùng transaction. Exchange lock owner → Device → grant, recheck
expiry/revoke/consumption, activate Device, insert credential digest và consume
grant nguyên tử. Production lấy clock expiry mới sau khi giữ cả ba lock; request
đang chờ không thể dùng timestamp trước lock để nhận grant đã hết hạn. Response
mất không cho reissue secret. Browser owner revoke-all
credential và grant chưa dùng, giữ Device để cấp grant mới cùng Device.
Single-credential/Device revoke được hỗ trợ; mỗi request đọc trạng thái hiện
tại. Không thêm automatic rotation/expiry cho bearer hay xóa audit row.

Local SQLite là authority state desktop riêng, không fallback database server.
Forward-only `0002_server_profiles.sql` thêm profile, enrollment ID/timestamp,
cleanup intent và replica/profile binding không bí mật; giữ nguyên
`0001_initial.sql`. Trigger immutable chặn rebind origin/owner/Device/profile;
replica legacy chưa bind không suy ra server từ Library ID. Bearer chỉ ở
platform SecretStore, key bằng opaque profile/credential identity; không có
column password, bearer, enrollment token, cookie hay credential export.

Prompt 37 status:

| Capability | Status |
|---|---|
| durable change journal | `VALIDATED` |
| device checkpoints/feed | `VALIDATED` |
| snapshot/rebaseline | `VALIDATED` |
| client mutation submission | `VALIDATED` |
| optimistic conflict detection | `VALIDATED` |
| durable conflict records | `IMPLEMENTED` |
| manual conflict inspection | `IMPLEMENTED` |
| explicit manual resolution | `IMPLEMENTED` |
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop inbound sync core | `VALIDATED` |
| desktop server profiles | `IMPLEMENTED` |
| device enrollment groundwork | `IMPLEMENTED` |
| device bearer authentication | `IMPLEMENTED` |
| secure desktop credential persistence | `IMPLEMENTED` |
| production HTTP SyncRemote | `IMPLEMENTED` |
| filesystem observation | `IMPLEMENTED` |
| durable outbound intent capture | `IMPLEMENTED` |
| automatic outbound mutation submission | `NOT IMPLEMENTED` |
| desktop GUI/pairing UX | `NOT IMPLEMENTED` |

Prompt 41 should add the next bounded backup capability only after this gate passes.

`20260829000000_backup_domain_snapshot_manifest.sql` adds only the durable
backup domain: owner/name-scoped `backup_sets` carrying retention metadata, the
immutable `backup_snapshots` capture lifecycle, and immutable
`backup_snapshot_nodes` manifest rows. The snapshot captures the current
logical namespace cut (journal epoch and head sequence) at commit time; the
manifest is written and completed in the same transaction as the capture, so a
partial snapshot can never become restorable. Snapshot captures are
retry-idempotent through `(owner_user_id, backup_set_id, operation_id)`.

The manifest stores logical Node/FileVersion references and safe content
metadata (byte length, SHA-256) only. No Object ID, ObjectReplica ID, storage
key, backend locator, staging handle, filesystem path, credential, or byte is
persisted here. Manifest rows have no foreign key back to mutable
Node/FileVersion/Object rows, so later renames, moves, Trash/purge, or content
replacement cannot alter or block a captured backup. An
append-only trigger blocks in-place UPDATE on manifest rows; only the capture
transaction inserts them. Expiration is a lifecycle state transition, not a
hidden DELETE. The later explicit prune execution releases only its authorized
retention pins while preserving the expired snapshot and manifest as audit
history. The migration adds no crawler, scheduled execution, incremental
planner, upload/restore execution, retention worker, pruning, or remote target.

`20260829000001_backup_content_retention.sql` adds only server-internal
`backup_snapshot_content_pins`: one durable canonical-Object retention
reference for each captured manifest content row. Pins are snapshot-owned and
may be released only through the later exact-plan authorized prune execution;
the snapshot and manifest remain preserved. Pins have no FK to mutable
Node/FileVersion rows and therefore survive metadata purge. A pin INSERT is
accepted only for a BUILDING snapshot
and immediately validates the exact manifest/FileVersion/Object mapping; pin
rows are append-only and direct release is rejected while the owning snapshot
exists. Snapshot lifecycle transitions are one-way, so a committed owner cannot
be reopened. Ordinary deletion of a COMPLETED or EXPIRED snapshot is also
rejected, while the existing cascade remains available for a later explicit
pruning protocol. The migration also makes snapshot completion reject a
manifest/pin count or mapping mismatch, requires a pinned Object to remain
referenceable during capture, and provides the Object-keyed index used by the
existing purge and GC rechecks. Snapshot expiry remains lifecycle metadata and
does not remove a pin, manifest row, Object, replica, or byte.
`20260829000002_backup_restore_plans.sql` adds the durable, owner-scoped
non-destructive restore-plan foundation. A plan captures one completed
snapshot's logical tree beneath a newly named destination directory, the
target library journal epoch/head used for staleness validation, a semantic
retry fingerprint, and immutable logical plan entries with durable planned
Node IDs. Creation performs a metadata-only recoverability preflight through
the retained pin, canonical Object, and verified-replica relations; it does
not require the historical live FileVersion row and does not read or write an
ObjectStore. The only plan lifecycle transition is `PLANNED` to `STALE` when
target evidence changes. The migration adds no restore execution, live
namespace mutation, overwrite/merge behavior, pin release, pruning, HTTP/UI,
or client protocol.
`20260829000003_backup_restore_execution.sql` adds the atomic restore execution
receipt and immutable per-entry execution evidence. It extends the plan
lifecycle with terminal `EXECUTED` and keeps `ASSEMBLING` transaction-local
through deferred sealing checks. One committed execution is uniquely bound to
one owner-scoped plan and cannot be deleted or rewritten.

`20260829000004_backup_prune_plans.sql` adds only durable, owner-scoped
retention-release preflight evidence for one `EXPIRED` snapshot. A sealed plan
contains logical manifest release entries plus private per-distinct-Object
reference-accounting evidence: all source-snapshot pins are prospective
releases, while live `file_versions` and pins owned by other snapshots remain
authoritative survivors. `ASSEMBLING` is transaction-local and deferred
sealing proves that every source pin and distinct retained Object is
represented before `PLANNED` can commit. Plans, entries, and impacts are
immutable except `PLANNED -> STALE` when validation detects source or reference
drift. The migration adds no pin release, snapshot/manifest deletion, GC
candidate/lease mutation, physical GC, ObjectStore operation, policy worker,
HTTP route, or client surface.

`20260829000005_backup_prune_execution.sql` adds an explicit, atomic execution
receipt for an accepted prune plan, authorizes only the plan-bound retention
pin release, and hands newly unreferenced canonical Objects to the existing GC
candidate pipeline. It does not delete ObjectStore bytes or expose physical
identity through the public backup domain.

`20260829000006_backup_retention_expiry_planning.sql` adds immutable,
owner-scoped snapshot-retention policy revisions and sealed, deterministic
snapshot-expiry plans. Policy values are limited to a positive newest-completed
floor and a positive minimum age. Planning ranks only COMPLETED snapshots by
duration is technically bounded to 10,000 Julian years so portable checked
timestamp subtraction cannot accept an unrepresentable duration. Planning
ranks by `committed_at DESC, id DESC`, records one of `KEEP_LATEST`, `KEEP_RECENT`,
`BLOCKED_ACTIVE_RESTORE_PLAN`, or `EXPIRE` for every cohort member, and binds
the decisions to the current immutable policy revision, a server-authoritative
evaluation time, and a versioned cohort/blocker fingerprint. A backup-set row
fence serializes policy revision allocation and coherent planning with snapshot
completion; active restore-plan rows are share-locked while observed. Plans and
entries are immutable after sealing except for `PLANNED -> STALE` validation.
This migration performs no snapshot expiry transition, retention-pin release,
prune operation, GC mutation, ObjectStore I/O, scheduler work, HTTP/client
surface, or manual-hold behavior.

`20260829000007_backup_expiry_execution.sql` adds explicit execution for a
sealed snapshot-expiry plan. Execution revalidates the original policy/cohort
basis, marks stale plans without replanning, persists one committed receipt and
per-entry evidence, and transitions only plan-approved snapshots from
`COMPLETED` to `EXPIRED`. It releases no retention pins, creates no prune plan,
performs no GC handoff, and performs no ObjectStore operation.

`20260829000008_backup_maintenance_runs.sql` adds the durable explicit manual
backup maintenance-run coordinator. A run binds the current retention-policy
revision at creation, stores owner-scoped idempotency evidence plus immutable
child operation identities, and records monotonic progress through snapshot
capture, expiry planning, expiry execution, and terminal completion or
staleness. The coordinator stores only logical child references; child tables
remain authoritative for snapshot, expiry-plan, and expiry-execution details.
It adds no scheduler/background worker, HTTP/client surface, automatic prune,
retention-pin release, GC handoff, physical GC, or ObjectStore I/O.

`20260902000000_backup_scheduling_domain.sql` adds the Prompt 61 durable
owner-scoped scheduling domain: one stable logical schedule per `BackupSet`, an
append-only immutable revision history, normalized daily/weekly local-time
configuration, explicit IANA timezone identity, a monotonic current-revision
pointer, and idempotency evidence for both changes and semantic no-ops. The
database scope is protected by composite ownership foreign keys, one-schedule-
per-set uniqueness, immutable-history triggers, and a monotonic current-pointer
trigger. It adds no scheduled-occurrence, job, lease, retry, catch-up,
execution, scheduler, worker, snapshot, maintenance, journal, ObjectStore,
HTTP, or client surface; automatic backups are not enabled by this migration.

`20260902000001_backup_schedule_occurrences.sql` is server migration 31. It
adds Prompt 62's dedicated `effective_from` schedule boundary and immutable
`backup_schedule_occurrences` ledger. Existing Prompt 61 schedules are
conservatively backfilled from their last durable `updated_at` transition.
Composite owner/BackupSet/schedule/revision foreign keys prevent forged scope;
the logical `(schedule_revision_id, local_calendar_date)` key and secondary
`(schedule_id, scheduled_for_utc)` key enforce exactly-once firing identity.
Occurrence UPDATE/DELETE is rejected. This migration adds no execution state,
worker, lease, retry/catch-up policy, snapshot, maintenance run, journal, sync,
ObjectStore, HTTP, or client surface; materialized does not mean executed.

`20260902000002_backup_schedule_handoffs.sql` is server migration 32. It adds
the minimal immutable `backup_schedule_occurrence_handoffs` relation for
Prompt 63: one already-materialized occurrence binds exactly once to one
canonical `backup_maintenance_runs` row. Composite owner/BackupSet/schedule/
occurrence/run foreign keys and unique constraints prevent cross-scope
forgery, occurrence reuse, and maintenance-run reuse; an append-only trigger
rejects UPDATE and DELETE. The relation stores only logical provenance and an
observed creation timestamp. It adds no execution state, worker, lease, retry,
poller, scheduler, snapshot, expiry, prune/GC, journal, sync, ObjectStore,
HTTP, or UI behavior; a handoff is not a completed backup.

`20260903000000_backup_misfire_policy.sql` is server migration 33. It extends
immutable schedule revisions with the closed `REPLAY_ONE_BY_ONE`/
`LATEST_ONLY` policy and a 60-through-2678400-second maximum-lateness bound;
existing rows receive the safe `LATEST_ONLY`/604800-second default without
changing revision IDs, numbers, timestamps, pointers, occurrences, handoffs,
or maintenance runs. Fingerprint constraints retain historical version 1
evidence and admit policy-aware version 2 evidence. The new immutable
`backup_schedule_misfire_skips` relation records one activation-scoped expired
range, with composite owner/BackupSet/schedule/revision foreign keys, policy
snapshot and activation validation, monotonic progress, bounded lookup indexes,
and UPDATE/DELETE rejection. It adds no daemon, poller, worker lease, retry,
maintenance advancement, snapshot, expiry, prune/GC, journal, sync,
ObjectStore, HTTP, client, or UI behavior.

`20260903000001_backup_scheduled_maintenance_claims.sql` is server migration
34. It adds the Prompt 66 durable `backup_scheduled_maintenance_claims`
relation: one claim authorizes exactly one canonical Prompt 49 transition from
one expected maintenance state (`CREATED`, `SNAPSHOT_CAPTURED`, or
`EXPIRY_PLANNED`) with the predetermined resulting state. Claim identity is
`(maintenance_run_id, expected_state)` with database uniqueness; lease tokens
are unique. Every lease carries an internal worker ID, an unpredictable token,
a `lease_generation` starting at 1, and a strictly ordered
acquired/expires pair; completion requires `completed_at` and `resulting_state`
together. Composite owner/`BackupSet`/schedule/occurrence/run foreign keys plus
an insert-time trigger fence provenance to a committed Prompt 63 handoff, so
manual runs can never gain a claim. An update trigger admits only
expiry-gated takeovers (generation N to N+1 with a fresh token and a new
interval acquired at or after the previous expiry) and completions sealing the
predetermined result with lease identity frozen; completed receipts reject
UPDATE and all rows reject DELETE. It adds no daemon, polling/heartbeat loop,
retry/backoff, scheduler, HTTP route, UI, SSE/WebSocket, notification, or
physical-storage identity; one explicit worker invocation still performs at
most one transition or recovery action.

`20260908000000_rebaseline_durable_snapshots.sql` is server migration 35. It
adds the Prompt 82 library-scoped `rebaseline_snapshots` header and immutable
`rebaseline_snapshot_entries` artifact. One `REPEATABLE READ` transaction
under the existing per-library namespace guard captures the typed journal epoch
and resume sequence, validates the Prompt 81 logical Node projection, inserts
the header with its exact entry count, and copies the logical rows set-wise.
The header and entries are therefore visible only after the one transaction
commits. The `(snapshot_id, node_id)` primary key is the deterministic keyset
page index; there is no OFFSET pagination or speculative cleanup index.

The transfer artifact stores only current logical Node fields already admitted
by `LogicalSnapshotNode`: ID, parent ID, name, kind, active/trashed state,
revision, and safe current-file version/length/SHA-256 metadata. It has no
device ID, checkpoint linkage, Object/ObjectReplica ID, storage key, backend
or filesystem locator, staging handle, credential, or file bytes. Header and
entry rewrites/direct deletes are rejected; a live-library deletion may
cascade the short-lived artifact only, never in the reverse direction. Logical
expiry is caller-observed and does not add a cleanup worker, retention policy,
or public deletion route.

Prompt 85 deliberately adds no server migration. Its original durable
checkpoint handoff used the immutable migration-35 snapshot header as proof and
the existing
`device_sync_checkpoints` row as the per-device destination. A short
owner/device-scoped transaction derives the Library and boundary from the
header, locks the canonical checkpoint, and applies only the documented
monotone-or-equal transition. Idempotency is header/checkpoint equality; no
handoff table, journal retention, snapshot cleanup, or new persistence surface
is introduced. The desktop client uses its existing schema-5
`rebaseline_applied_handoffs` marker and `replicas` cursor fields, so no
`0006` migration is required.

`20260910000000_sync_retention_handoff_proofs.sql` is server migration 36. It
formalizes the existing `libraries.minimum_retained_sequence` as the highest
current-epoch sequence physically compacted through and adds the small,
immutable `rebaseline_snapshot_handoff_proofs` relation. Migration 36 backfills
one proof for every migration-35 snapshot with the same snapshot ID,
owner/Library, journal epoch/boundary, and snapshot timestamps; the proof
deadline is 30 days after payload expiry. The proof has no foreign key to the
payload header, so bounded deletion of an expired `rebaseline_snapshots` row
and its cascaded entries cannot remove handoff authority.

New snapshot creation writes payload and proof in one application transaction.
Prompt 85 handoff now reads the proof for both present and already-pruned
payloads. The migration adds the minimum indexes for Library/epoch boundary
pinning, snapshot expiry selection, and proof expiry selection, plus immutable
proof and transaction-local controlled-delete guards. It adds no endpoint,
daemon, timer, retry, client-local migration, or automatic recovery behavior.
