# P029 Security Audit and Revocation Traceability Qualification

**Status:** provisional reference qualification only. The authoritative P001–P060 roadmap was not found in this checkout, workspace, or GitHub code search; reconcile the roadmap before treating this as the official P029 scope. P004-D05 and P004-D06 remain `RECOMMENDED_PENDING_APPROVAL`.

## Scope and trust boundaries

P029 adds a typed evidence model, deterministic verifier and test-only PostgreSQL extension around the existing P025–P027 reference work. It has no production callers, API route, production migration, role-policy change, token, credential, approval endpoint, or native effect. The P027 disposable schema remains the only authority fixture. P029 tables contain evidence and checkpoints only; they do not issue authority or decide access.

Observed production boundaries remain those already documented in P025/P026: browser sessions are authenticated and revoked through `AuthenticationService` and metadata; device credentials use the active owner/device and revocation state; the API binds owner and device scope before repository access; repository ownership checks protect logical resources. The P025/P026 evaluator is reference-only. P027 is separately a PostgreSQL authority-ledger reference in test schemas, not production RBAC. No authorization behavior in these production call sites changed.

The P027 reference transaction reloads persisted actors, session revocation, policy revision, ownership, grants and tombstones under a serializable global policy fence. Approval consumption, synthetic operation, P027 audit row and outbox row share one database transaction. The P029 test adapter inserts typed evidence in that same transaction. It does not expand P027 into a production ledger or claim that all production authorization paths already use that fence.

## Version 1 evidence schema

`authorization_reference::audit_v1` defines a closed set of event types, principal kinds, operations, outcomes, denial categories and timestamp provenance. Each record binds:

- schema version and event ID;
- trusted actor reference, principal kind and optional device reference;
- owner scope and optional logical resource reference;
- effective operation, policy revision and result/denial category;
- owner-stream sequence and global ordering reference into P027's existing audit sequence;
- operation and correlation IDs, database-clock timestamp provenance, and optional related approval/revocation reference;
- previous digest and current SHA-256 digest.

The schema has no credential, bearer, password, recovery-secret, session-token, request-body or arbitrary metadata field. Identifiers are bounded stable references, not credential values. Session-revocation evidence points to the P027 audit sequence rather than copying the session identifier into P029. The PostgreSQL fixture applies matching size/character bounds and rejects unsupported event types, outcomes and categories. Unknown version or enum values fail closed. Foreign and unknown resources share a generic denial category and denial records omit the resource ID.

## Ordering, durability and integrity

The P029 fixture is installed only into a generated P027 schema. A per-owner evidence head is row-locked in the active P027 transaction. The writer allocates the next transactional sequence, reads the timestamp from PostgreSQL, references the P027 audit sequence for global ordering, hashes the length-prefixed typed fields with the prior digest, inserts the immutable event, and advances the checkpoint. The head trigger accepts exactly one next event whose previous digest matches the current head. The P027 global policy fence serializes participating authority mutations; the global P027 audit sequence remains the cross-owner order reference.

Authority mutation, approval consumption, P027 audit/outbox records, P029 event and checkpoint either commit together or roll back together. A rejected request may commit a typed denial record but cannot create a committed-success event. Duplicate completed operations recover the existing outcome and do not create another committed event. Audit insertion errors abort the transaction; the test retries the same operation only after the failure is removed.

The verifier checks supported schema, scope, contiguous owner sequence, increasing P027 order reference, unique event IDs, unique committed authorization operation, prior-digest linkage, current digest, record count and expected head digest. It cross-checks both directions: P029 committed, denial, approval-consumption and revocation rows must match P027 authority history, and relevant P027 committed, denied and revocation history must have matching P029 evidence. A test removes a success event and rewrites the local checkpoint; the missing evidence is still detected against the P027 record. Pages return at most 100 rows; complete verification rejects a stream over 10,000 records, a truncated stream, an invalid cursor or an incomplete checkpoint with an explicit error rather than returning partial success.

The digest chain detects accidental or uncoordinated record edits relative to a separately retained checkpoint. In this reference, both records and checkpoint reside in the same PostgreSQL database. A database administrator able to rewrite the event rows, checkpoint and P027 audit can replace the entire chain. SQL immutability triggers constrain ordinary writes but do not make evidence administrator-resistant. No independent signature, external anchor, immutable storage, delivery guarantee or production audit-retention policy is implemented.

## Revocation and retry behavior

The reference path resolves current session, actor/device revocation, grant ancestry, policy revision and ownership after taking the P027 fence. A committed grant tombstone blocks later authorization, including inherited delegated grants. A stale serializable snapshot fails/retries under the P027 bounded retry classifier; a retry reloads authority. A committed retry recovery returns the old outcome without consuming approval or recording a second committed success.

P029's PostgreSQL tests qualify transactionally written approval consumption and session/device/grant revocation, post-revocation denial, a device revoke waiting for an in-flight operation and its evidence, concurrent grant use/revocation ordering, duplicate operation recovery, a deliberately dropped PostgreSQL `COMMIT` response followed by idempotent recovery, audit/outbox storage failure, backend termination, and scoped verified reads. The inherited P027 suite independently exercises session/actor/device revocation, policy and ownership revision fences, approval replay, delegation/revocation races, PostgreSQL process termination, subprocess exit and database restart. These tests are fixture semantics. They do not establish a production call-site freshness guarantee or invalidate every production cache.

## Evidence access and limits

The reference read path accepts an owner scope resolved from trusted authority state, uses a repeatable-read snapshot, orders deterministically, validates all records against the checkpoint and P027 authority rows, and then returns a bounded page. It rejects caller/requested scope mismatch, unsupported records, missing rows, ordering gaps, duplicate IDs, digest mismatch, inconsistent P027 links and oversized histories. No production API, export, admin UI, retention, legal-access or cross-owner read mechanism is introduced.

The P029 qualification also prepares a committed authority mutation and its evidence, restarts the disposable PostgreSQL 17 server container, reconnects and verifies the records. This qualifies persistence across that server restart only; it does not prove power-loss recovery, storage-controller behavior, backup restoration, or hostile-administrator resistance. Hosted status is reported only for the exact pushed source head.

## Security gate state

| Gate | State | Evidence/remaining limit |
|---|---|---|
| Typed versioned reference schema | Implemented | Closed typed enums and schema-version rejection; P029 verifier tests. |
| Transactional evidence ordering | Reference-only | PostgreSQL test adapter binds evidence to P027 mutations; no production caller. |
| Revocation traceability | Reference-only | P027 fence, revocation and ordering fixture tests; no global production freshness claim. |
| Completeness/integrity checks | Implemented in reference | Checkpoint, sequence, digest and P027 linkage checks; checkpoint shares the database trust domain. |
| Redaction/scoped access | Reference-only | Minimal typed fields, bounded identifiers, foreign/unknown denial, owner equality and bounded page; no production audit reader. |
| P025–P027 compatibility | PASS in the focused exact-head Linux/Windows workflow at source SHA `a9142118ce620017c2798128b590d7a1afae3d03`; PR #190 records the latest head result. | No policy, auth, SQL authority semantics or legacy route changed. |
| Production security | **BLOCKED** | P004 decisions, retention/privacy/access, independent integrity anchor, migrations/rollback, call-site review and independent security review remain outstanding. |

The reference implementation gate is `REFERENCE_QUALIFIED` for the source SHA above. The P029 roadmap scope remains provisional; PR #190 records the latest exact-head hosted result. This qualification does not satisfy the production security gate.

No claim is made that database hashes prove approval, that a database administrator cannot alter evidence, or that external effects are exactly-once. Production RBAC, privileged tokens, API endpoints, audit export and policy changes remain disabled.
