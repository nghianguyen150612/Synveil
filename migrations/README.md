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
