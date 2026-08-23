# Contributing to Synveil architecture

Status: **Normative process**

This document prevents parallel contributors and coding agents from creating
locally reasonable but globally incompatible storage, sync, API, and data
contracts.

## Authority and precedence

When artifacts disagree, stop implementation and resolve the higher-level
contract in this order:

```text
Accepted Architecture Decision Records
    ↓
Domain and protocol specifications
    ↓
Reviewed OpenAPI contract
    ↓
Database migrations and storage-format versions
    ↓
Implementation and generated clients
    ↓
Examples and prose that are not marked normative
```

Later artifacts may add detail but may not contradict earlier ones. Existing
accepted migrations and on-disk formats also create compatibility obligations;
an ADR cannot make deployed data disappear.

If an accepted ADR and a protocol spec disagree, open an architecture change
before writing code. If the implementation differs from all specifications,
the difference is a defect unless an approved migration says otherwise.

## Baseline reading order

For a new product or platform decision, read [PRODUCT.md](PRODUCT.md),
[PLATFORM.md](PLATFORM.md), [ARCHITECTURE.md](ARCHITECTURE.md), and the relevant
domain/protocol specification before selecting an implementation. `PLATFORM.md`
defines the user-facing Personal/Home and Advanced/Server boundary, host
support, service lifecycle ports, filesystem capabilities, pairing, remote
access, and recovery expectations; it does not move OS concerns into domain
logic.

## Frozen baseline

The blueprint fixes these defaults for Phase 0 and Phase 1:

1. A modular Rust monolith exposes the API and shares domain crates with a Rust
   worker. Python is a separate optional AI runtime.
2. PostgreSQL is authoritative for metadata and transactional state. File
   contents are normal object-store objects, not database BLOBs.
3. Public domain IDs are opaque UUIDv7 strings. Clients must never infer time,
   ownership, storage paths, or authorization from an ID.
4. API paths start at `/api/v1`; JSON uses UTC RFC 3339 timestamps, stable
   machine error codes, keyset cursors, and conditional mutation.
5. Each synchronization `Library` has a transactionally incremented change
   sequence. A cursor is opaque and versioned; clients do not manufacture it.
6. Successful content commits reference a durable, verified object and append
   the metadata change, audit record, and durable outbox work atomically.
7. SHA-256 is the initial canonical plaintext integrity and whole-object
   deduplication hash. Storage keys are opaque and do not expose hashes or user
   paths. Deduplication does not cross an ownership/dedup domain by default.
8. Backup snapshots and sync state are different domains. Missing backup input
   does not emit a live deletion.
9. Optional workers, AI, thumbnails, OCR, search enrichment, and Forgejo
   polling cannot be synchronous prerequisites for core storage correctness.
10. Synveil has two deployment profiles over the same protocol and data model:
    Personal/Home is the planned guided/native profile for ordinary users, and
    Advanced/Server is the operator-controlled profile where Docker Compose or
    another reviewed topology is supported. Kubernetes, a message broker,
    Redis, and a service mesh are not Phase 0 dependencies.
11. Windows, macOS, Linux Desktop, and Linux Server are first-class host targets
    for the platform contract. Android, iPhone, and iPad remain future client
    targets; no OS service manager, shell, or desktop assumption may leak into
    the domain core.
12. Storage is selected through a capability-driven adapter contract. NTFS,
    ReFS, APFS, Btrfs, ext4, XFS, future ZFS, NAS, and object storage are
    evaluated by discovered capabilities; Btrfs/WinBtrfs are optional
    accelerators, never correctness requirements.
13. All product features remain at their documented `PLANNED` or
    `EXPERIMENTAL` status and never become `IMPLEMENTED` until code, required
    tests, operations guidance, and status documentation have passed their
    promotion gates.
14. The existing MIT license stays legally operative unless the owner
    explicitly approves and performs a compatible relicensing process.

Changing a frozen choice requires an ADR with migration and compatibility
impact. A scoped experiment may be documented without changing the default.

## Shared terminology

Technical identifiers remain identical in English and Vietnamese documents.

| Term | Normative meaning |
|---|---|
| `Node` | A user-visible file or directory identity in a `Library`. |
| `FileVersion` | An immutable metadata record binding a file revision to one canonical `Object`. |
| `Object` | The immutable canonical plaintext content identity and logical lifecycle record inside one dedup domain. |
| `ObjectReplica` | One immutable backend/key-specific stored representation of an `Object`, with codec, stored checksum, and replica state. |
| `Library` | A synchronization namespace and ownership/policy boundary with one ordered journal. |
| `ChangeEvent` | A durable committed fact needed by clients to advance a `SyncCursor`. |
| `SyncCursor` | An opaque server token representing a position and epoch in a library journal. |
| `UploadSession` | A resumable, idempotent staging workflow; not a visible file until committed. |
| `BackupSet` | A device-scoped definition of protected sources and retention policy. |
| `BackupSnapshot` | An immutable committed manifest view; `BUILDING` snapshots are not restorable. |
| `dedup domain` | The boundary inside which equal plaintext objects may reuse physical bytes. |
| `outbox` | Work recorded in the same transaction as a core mutation and delivered at least once. |
| `Personal/Home` | The guided, self-hosted deployment profile that hides routine infrastructure choices while preserving the same server protocol and data model. |
| `Advanced/Server` | The operator-controlled deployment profile for Compose, external PostgreSQL, custom proxy/TLS, NAS/S3, CLI, and other explicit infrastructure choices. |
| `PlatformRuntime` | A port for process, service, secret, discovery, update, diagnostics, and storage-host capabilities; it is not a domain authorization layer. |
| `ServiceLifecycle` | The start, stop, restart, readiness, upgrade, and shutdown contract implemented by Windows Service, launchd, systemd, or another adapter. |
| `StorageCapabilities` | Evidence-backed operations a selected storage backend can safely provide; missing acceleration falls back to portable behavior. |
| `pairing` | A short-lived, explicit device-enrollment exchange that is scoped, revocable, replay-resistant, and separate from long-lived credential storage. |

The domain model owns exact field definitions. Prose must not introduce
alternate meanings such as treating `Object` as a user-visible file.

## Status taxonomy

- `IMPLEMENTED`: reachable implementation, required automated validation,
  operational handling, and docs all pass.
- `IN PROGRESS`: scoped work exists but its exit gate has not passed.
- `PLANNED`: a reviewed intended capability without a completed implementation.
- `EXPERIMENTAL`: unstable research; data formats and API are not promised.
- `NON-GOAL`: explicitly excluded from the relevant horizon.

Moving a label is a reviewable change. A mock, UI shell, migration alone, or
happy-path endpoint is not sufficient evidence for `IMPLEMENTED`.

## Architectural change workflow

1. Name the invariant or limitation being changed.
2. Identify affected ADRs, specifications, API operations, migrations, stored
   representations, clients, tests, threat controls, and bilingual docs.
3. Write or amend an ADR. Include alternatives, compatibility, migration,
   rollback limits, observability, and security consequences.
4. Obtain an architecture owner and relevant domain owner review.
5. Update domain/protocol documents in both languages.
6. Update OpenAPI before or with implementation; generate clients only from a
   reviewed contract.
7. Add migration and compatibility tests before deploying a new writer.
8. Roll out readers-before-writers when mixed versions can coexist.
9. Record the decision and remove contradictory examples.

An urgent production fix may precede prose only when it protects data or
security. It still needs a follow-up ADR and contract reconciliation before the
next feature release.

## `OPEN DECISION` protocol

Use this exact marker when a choice is intentionally unresolved:

```text
OPEN DECISION OD-NNN: short title
Owner: workstream or named role
Needed by: phase gate
Options: bounded alternatives
Recommendation: current preferred option and why
Decision evidence: test, benchmark, threat review, or product input required
```

An open decision must not block earlier work unless its `Needed by` gate has
arrived. Implementers may not quietly choose an option whose stored or public
contract escapes the experimental boundary.

## Task contract for implementation agents

Every coding task is issued independently with all fields below. A task that
cannot fill a prerequisite or exit gate is not ready to dispatch.

```markdown
# Role
The domain responsibility and review obligations.

## Context
Accepted ADRs/spec sections and current repository evidence.

## Prerequisite gate
Concrete artifacts/tests that must already pass.

## Objective
One observable outcome.

## Exact scope
Included behavior and edge cases.

## Expected files/components
Owned paths; shared paths require named coordination.

## Implementation constraints
Invariants, compatibility, security, performance, and dependency limits.

## Tests
Unit, integration, recovery, protocol, security, or benchmarks required.

## Validation
Exact commands and manual evidence.

## Forbidden changes
Unowned modules, public contracts, migrations, formats, or scope expansions.

## Exit gate
Evidence that changes the task from in progress to complete.

## Required report
Files changed, tests/results, assumptions, risks, migrations, and follow-ups.
```

The task must be small enough for one accountable owner. Split tasks when they
would change unrelated contracts, and sequence them when one consumes another's
unstable output.

## Parallel work rules

Parallel work is safe when contributors own different modules and consume an
already reviewed contract. It is unsafe to design IDs, errors, cursor semantics,
storage formats, database relationships, or authorization independently.

Before parallel work begins:

- nominate one contract owner;
- list path ownership and shared files;
- freeze the relevant ADR/spec revision;
- define fixtures or contract tests used by every side;
- identify the integration task and rollback point.

Database migrations are serialized and immutable after release. OpenAPI edits
have one integrator. Generated files are never hand-edited in parallel.

## Required gates for every phase

- **Architecture:** accepted decision and no unresolved blocking question.
- **Security:** threats, authorization, secret handling, abuse limits, and audit
  effects reviewed.
- **Correctness:** invariants and failure/retry behavior tested.
- **Performance:** phase-relevant methodology and regression limit recorded;
  no invented marketing claims.
- **Operations:** health, logs, metrics, backup/restore, and upgrade behavior
  documented.
- **Documentation:** English and Vietnamese meaning aligned; public status
  accurate.
- **Next phase:** integration suite green and no unresolved high-severity data
  loss or authorization defect.

## Documentation parity

English files under `docs/en` are the normative tie-breaker until the team can
review both editions simultaneously. Vietnamese files under `docs/vi` must
convey the same decisions, warnings, state names, IDs, error codes, and gates;
they are not shortened summaries. ADRs contain both languages in one file so a
decision cannot acquire two statuses.

Any change to a core English specification must include its Vietnamese peer in
the same documentation gate. If a translation is temporarily delayed, mark
both files visibly and block release—not ordinary local drafting—until parity
is restored.

## Review checklist

- Does the change preserve storage, sync, backup, version, and restore
  invariants under crash and retry?
- Can a lost response be replayed safely?
- Does authorization derive from the authenticated principal and resource
  relationship rather than possession of an ID?
- Are optional services outside the critical path?
- Are size, memory, quota, and disk-full behaviors bounded?
- Are database/object-store split-brain outcomes repaired or quarantined?
- Can an old client detect incompatibility and recover without data loss?
- Are user-visible status claims supported by evidence?
- Does the change preserve the same protocol/data model across Personal/Home and
  Advanced/Server?
- Does every first-class host have a reviewed installer/service/update,
  uninstall, migration, and recovery path, or is the capability explicitly
  `PLANNED`?
- Are filesystem-specific accelerators behind `StorageCapabilities`, with a
  portable correctness path when they are absent?
- Can a nontechnical user complete the core flow without a terminal, while an
  advanced operator can still reach explicit infrastructure controls?
- Are both language editions and links valid?
