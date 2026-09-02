# Synveil team and agent execution plan

Status: **Normative planning contract**

This document turns [ROADMAP.md](ROADMAP.md) into work that human developers
and implementation agents can execute without independently redefining the
system. It covers all phases, but it does not authorize work whose prerequisite
gate has not passed. Capabilities remain at their documented `PLANNED` or
`EXPERIMENTAL` status and never become `IMPLEMENTED` until implementation,
validation, operations guidance, and bilingual documentation all pass their
promotion gate.

## Operating model

Synveil uses contract-first, evidence-gated delivery:

```text
decision/spec task
    ↓ reviewed contract + fixtures
bounded implementation tasks
    ↓ component evidence
integration/recovery/security task
    ↓ phase evidence bundle
gate owner promotion
```

An implementation agent owns one observable outcome and a bounded path set. An
agent does not self-expand scope to “finish the feature,” silently resolve an
`OPEN DECISION`, change an accepted ADR, or modify a released migration. If a
contract is insufficient, the implementation task stops at a written contract
gap and routes a separate architecture task.

One named person or agent is **accountable** for each work package. Reviewers
are responsible for independent evidence, but the accountable owner remains
responsible for resolving findings and reporting residual risk.

## Workstreams and standing responsibilities

| Workstream | Owns | Must review |
|---|---|---|
| Architecture / Contracts + Platform / Distribution | ADR process, domain/protocol invariants, platform/runtime boundary, deployment modes, capability contracts, contract compatibility | Any ID, event, cursor, object format, public API, service-manager, installer, update, migration, or platform-boundary change |
| Rust Backend | Axum transport, application commands/queries, domain composition, stable errors | API behavior, bounded streaming, transaction orchestration |
| Database / Jobs | PostgreSQL schema, SQLx repositories, isolation/locks, migrations, outbox/jobs | Every transactional invariant, backfill, retention query, and upgrade |
| Storage / Uploads + Storage Platform | `ObjectStore`, local/S3 adapters, filesystem capability probes, storage picker, staging, checksums, object lifecycle, GC | Any content-write, range-read, compression, dedup, storage capability, or storage migration path |
| Sync | Journal, cursors, conflicts, initial scan, client conformance | Every live namespace mutation and client protocol change |
| Backup / Restore | Sets, snapshots, manifests, retention, restore operations | Any feature that claims historical protection or deletes retained references |
| Auth / Security | Credentials, authorization policy, shares, threat model, audit, abuse limits | Every external boundary, secret, parser, public link, integration, AI egress |
| Web + UX / Accessibility | React/Vite app, generated API types, accessible progressive-disclosure UX, transfer/recovery UX | User-visible status, terminology, destructive confirmation, onboarding, health, recovery and conflict UX |
| DevOps / Release + Installer / Updater + Networking / Connectivity | CI, images, Compose/Caddy, native packaging, service adapters, configuration, observability, signed release/update, remote-access connectivity | Deployment, health/readiness, backup runbooks, artifact provenance, service privilege, pairing, proxy/TLS, relay and migration behavior |
| QA / Reliability | Test architecture, fixtures, model/property/fuzz tests, E2E, failure injection | Phase evidence and regression acceptance; does not merely rerun happy paths |
| Documentation | English/Vietnamese parity, operator/developer/user docs, status taxonomy | Every public contract, feature promotion, migration and risk disclosure |
| Clients + Desktop / Mobile | Reference client, Windows/macOS/Linux desktop adapters, future Android/iPhone/iPad clients, local state, capability negotiation | Protocol fixtures, OS naming/filesystem semantics, background limits, credential storage, pairing, sync/backup parity |
| Photos | Photo metadata, derivatives, timeline/albums, PhotoKit contract | Parser isolation, originals, EXIF/location, source-deletion semantics |
| AI / Privacy | Python runtime, OCR/embedding/index provenance, modes and provider policy | Every derived-data lifecycle, remote request, model/license and AI search ACL |
| Integrations | Connector interface, Forgejo, webhooks/polling, repository backup | SSRF/credential policy, staleness, external compatibility and restore |
| Product / Project owner | Product scope, gate priority, ownership model, license decision | User promises, non-goals, paid/open split, unresolved policy trade-offs |

Small teams may assign several workstreams to one person. The responsibility
boundaries still apply, and a data-safety or authorization gate receives an
independent reviewer wherever practical.

## Authority and single-writer rules

The precedence in
[CONTRIBUTING_ARCHITECTURE.md](CONTRIBUTING_ARCHITECTURE.md) is mandatory:
accepted ADRs, domain/protocol specs, OpenAPI, migrations/formats,
implementation, then non-normative examples.

- ADRs and each protocol spec have one contract owner during a change.
- `api/openapi.yaml` has one integrator per public-contract change set.
- Migration sequence allocation is serialized. Released migration contents are
  immutable; correction is a new migration.
- Generated clients/types are produced from a recorded source and never edited
  by hand.
- Shared test fixtures are versioned and changed only with all consumers in the
  same integration plan.
- A task touching a shared authority file lists every downstream consumer and a
  merge order before work starts.
- UUIDv7 identifies entities; it is never substituted for the per-`Library`
  ordered change sequence.
- `ChangeEvent`, internal outbox/job, and `AuditEvent` are separate contracts
  and must not be collapsed by an implementation task.

## Standard agent task contract

Every future coding, documentation, review, or operations task must use the
following complete prompt. Bracketed guidance is replaced with concrete
values; it is not left for the agent to infer.

```markdown
# Task ID and title
[Stable phase/workstream identifier and one outcome]

## Role
[Domain responsibility, accountable owner, required reviewers]

## Context
[Exact accepted ADRs/spec headings, repository evidence, relevant prior task
reports, and current feature status]

## Prerequisite gate
[Named gate token plus exact artifacts/tests that already pass]

## Objective
[One externally observable outcome]

## Exact scope
[Included state transitions, inputs, errors, retries, edge/failure cases]

## Expected files/components
[Allowed owned paths; identify shared files and their integrator]

## Implementation constraints
[Domain invariants, transaction/durability boundary, auth/privacy limits,
compatibility, resource bounds, permitted dependencies]

## Tests
[Named unit, integration, conformance, property/fuzz, crash, security,
performance, E2E, and upgrade cases required by this task]

## Validation
[Exact commands, environment, fixtures, manual/visual checks, and expected
evidence format]

## Forbidden changes
[Unowned paths, ADR/API/schema/format/status changes, refactors, dependencies,
or scope expansions this task may not make]

## Exit gate
[Measurable outcome and reviewer evidence required for completion]

## Required report
[Result; exact files; tests with pass/fail/skip; migrations/formats; assumptions;
security/privacy impact; performance evidence; residual risks; follow-ups]
```

### Dispatch checklist

A coordinator dispatches a task only when:

1. the prerequisite gate is recorded as passed;
2. every blocking `OPEN DECISION` is closed or the task is explicitly confined
   to a disposable experiment;
3. expected and forbidden paths do not conflict with another active task;
4. input contract revision and fixtures are pinned;
5. failure and abuse cases are part of scope, not deferred as generic QA;
6. a reviewer/integration owner and merge order are named;
7. the task is small enough to roll back without discarding unrelated work.

### Required completion report

“Done” is not a valid report. The agent reports:

- `COMPLETE`, `PARTIAL`, or `BLOCKED` against the stated exit gate;
- exact files added/modified/deleted;
- migrations, public API, event/schema, stored-format and configuration impact;
- commands run and exact pass/fail/skip counts where tools provide them;
- untested assumptions and environmental limits;
- security, privacy, durability, performance and compatibility findings;
- any new `OPEN DECISION`, with owner and needed-by gate;
- the single recommended follow-up task, not an unsolicited implementation.

The reviewer reproduces critical evidence or explicitly records why it could
not. A task with required skipped tests remains partial.

## Parallel execution and integration

### Safe parallel work

- UI components and API implementation may work in parallel only after
  OpenAPI/error/idempotency fixtures are reviewed.
- Object-store adapters may work in parallel against one conformance suite;
  only one task edits the trait/capability contract.
- API and worker handlers may work in parallel after database/job schemas and
  lease semantics are frozen.
- English-to-Vietnamese documentation work follows a frozen semantic revision;
  both editions join the same release gate.
- Optional AI, Photos, and Forgejo work can run independently after their
  canonical input/event contracts and privacy boundaries are fixed.

### Work that must be serialized

- migration numbering and domain relationship changes;
- change-journal allocation, cursor and conflict semantics;
- object commit/finalization and GC eligibility rules;
- auth/session/share capability formats;
- release manifest and version promotion;
- resolution of a shared blocking ADR or `OPEN DECISION`.

### Integration sequence

1. Contract owner publishes the reviewed revision and fixtures.
2. Component owners implement on isolated branches/worktrees or non-overlapping
   paths.
3. Component tests pass against the pinned fixture.
4. One integration owner combines changes in dependency order, regenerates
   artifacts, and runs cross-domain tests.
5. Security/QA independently exercises negative and failure paths.
6. Documentation and operations owners reconcile status and runbooks.
7. Gate owner records evidence and promotes—or rejects—the phase token.

## Phase execution plans

Every phase below names all required planning fields. Detailed scenario
matrices belong to [TESTING.md](TESTING.md); threat controls belong to
[SECURITY.md](SECURITY.md); deployment evidence belongs to
[DEPLOYMENT.md](DEPLOYMENT.md).

The phase prose below uses `SV-G*` labels for the expanded roadmap's internal
evidence checkpoints. Where it says `Prerequisite gate` or `Next-phase gate`
with an `SV-G*` value, read it together with the canonical master-gate mapping
in the later section; an `SV-G*` value never authorizes promotion by itself.

### Phase 0 — Repository and architectural foundation

**Objective:** Create a secure, observable, contract-enforcing monorepo and
feature-empty runtime skeleton without making Compose the product definition.

**Prerequisite gate:** Blueprint/ADR review complete; repository audit accepted;
Phase 0 `OPEN DECISION` owners assigned.

**Responsible workstreams:** Architecture / Contracts + Platform / Distribution
accountable; Rust Backend, Database, Web + UX / Accessibility, DevOps +
Installer / Updater + Networking / Connectivity, Security, QA, Documentation,
Storage Platform, and project owner responsible for bounded packages.

**Tasks:**

1. Freeze naming, error, UUIDv7, timestamp, keyset, idempotency, migration,
   configuration and feature-status conventions.
2. Create Rust workspace and API/worker composition roots with dependency
   direction checks; add only crates with real Phase 0 content.
3. Create strict React/TypeScript/Vite workspace and generated/validated API
   type pipeline.
4. Create PostgreSQL migration runner plus jobs/outbox schema skeleton and
   advisory-lock/checksum rules.
5. Create configuration validation, liveness/readiness, tracing/log redaction,
   and a one-time bootstrap skeleton.
6. Create Compose/Caddy developer and Advanced / Server profiles with private
   networks and persistent-volume declarations; do not label them Personal /
   Home onboarding.
7. Freeze Personal / Home versus Advanced / Server, platform-runtime and
   service-lifecycle ports, filesystem capability probes, progressive-disclosure
   terminology, pairing, health-layer, remote-access, update, uninstall, and
   migration/recovery contracts in paired documentation and ADRs.
8. Add CI for formatting, lint, unit/integration, migration, docs links,
   dependency/license, secret and image checks.
9. Record the owner decision process for MIT versus a future license split; do
   not change the license without authorization.

**Allowed parallel work:** Rust, web, docs, and deployment skeletons may run in
parallel after path owners and configuration/API conventions are fixed. One
integrator owns OpenAPI; one owns migrations.

**Dependencies:** No product code; toolchain/runtime version choices, ADRs,
portable name decision, filesystem durability test plan, and contributor
license decision process.

**Expected files/components:** root workspace/toolchain files; `bins/`; initial
owned `crates/`; `apps/web/`; `api/openapi.yaml`; `migrations/`; `deploy/`;
CI configuration; scripts; tests; paired docs. Do not create empty future
client trees.

**Tests:** Clean build; dependency-direction check; error/ID/config unit tests;
migration apply/reapply/concurrency test; Postgres integration; one-time
bootstrap race; health/log redaction; Compose smoke; web type/lint/build; docs
links; secret/dependency/license scan.

**Definition of done:** Clean checkout passes CI; Compose reaches readiness;
bootstrap can be safely completed once; no secret/manual DB edit is required;
status remains truthful.

**Security gate:** Threat boundaries, secret sources, token/log rules, least
privilege, bootstrap close/race behavior, dependency policy and no unaccepted
critical finding are reviewed.

**Performance gate:** Baseline build/start/health and no-op request measurements
are recorded with environment; event-loop and DB pool instrumentation exists.

**Documentation gate:** Architecture, contributing rules, deployment skeleton,
testing commands, configuration and English/Vietnamese contract parity are
linked and accurate.

**Next-phase gate:** Record `SV-G0-FOUNDATION`; only then dispatch Phase 1
schema/storage/auth tasks.

### Phase 0A — Cross-platform foundation

**Objective:** Freeze the shared protocol/data-model/platform boundary before
implementation tasks choose a host-specific service or storage behavior.

**Prerequisite gate:** Blueprint/ADR review and `SYNVEIL_CONTRACTS_READY`.

**Responsible workstreams:** Platform / Distribution and Architecture / Contracts
accountable; Storage Platform, Clients + Desktop / Mobile, Web + UX /
Accessibility, Security, QA, Documentation, and project owner review.

**Tasks:**

1. Maintain the English/Vietnamese platform contract for Windows, macOS, Linux
   Desktop, Linux Server, and future Android/iPhone/iPad clients.
2. Freeze `PlatformRuntime`, `ServiceLifecycle`, `SecretStore`, storage
   discovery, update, diagnostics, and network-discovery ports; keep Windows
   Service, launchd, systemd, and Compose in adapters.
3. Freeze `StorageBackend → StorageCapabilities`, portable fallback behavior,
   filesystem/NAS/object-store capability fixtures, and optional Btrfs/WinBtrfs
   acceleration.
4. Freeze Personal / Home and Advanced / Server onboarding, pairing, remote
   access layers, user/admin/developer diagnostics, and no-terminal core flows.
5. Add the contract/error/accessibility/recovery evidence required by
   `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` without creating implementation
   directories for future clients.

**Definition of done:** The same API, domain model, storage correctness rules,
and stable error contract serve both modes; the gate is not a claim that a
native installer or client exists.

**Next-phase gate:** `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY`.

### Phase 0B — Distribution and recovery foundation

**Objective:** Turn the platform contract into an evidence-ready installation,
service, update, uninstall, migration, and recovery plan.

**Prerequisite gate:** `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY`.

**Responsible workstreams:** Installer / Updater + Networking / Connectivity
and Platform / Distribution accountable; Database / Jobs, Storage Platform,
Backup / Restore, Security, QA, Clients + Desktop / Mobile, Web + UX /
Accessibility, DevOps, Documentation, and project owner review.

**Tasks:**

1. Define the native/guided install preflight and storage picker for each
   first-class host profile, including data-preserving failure behavior.
2. Define managed PostgreSQL provision/discover/start/backup/restore/upgrade
   behavior without introducing SQLite as a second product model.
3. Define signed release/update verification, service crash/reboot/sleep
   recovery, health translation, and bounded automatic maintenance.
4. Define uninstall/reinstall retention choices and inspect/plan/validate/
   execute/verify machine migration and recovery fixtures.
5. Build the release-lab support matrix and the documentation-only gate bundle;
   leave package-format and relay decisions open until their evidence exists.

**Definition of done:** Every first-class host has a stated evidence path for
installation, service lifecycle, health, update, uninstall, migration, and
recovery; unsupported environments are labeled rather than implied.

**Next-phase gate:** Contributes to `SYNVEIL_FOUNDATION_READY`; it does not
authorize a supported native package before the platform-specific lab passes.

### Phase 1 — Storage foundation

**Objective:** Provide authenticated logical files/folders and bounded streaming
content I/O on the local production adapter.

**Prerequisite gate:** `SYNVEIL_FOUNDATION_READY` after
`SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` and `SV-G0-FOUNDATION`; portable name
and local durability decisions closed.

**Responsible workstreams:** Storage / Uploads and Rust Backend accountable;
Database, Auth / Security, Web, QA, DevOps, Documentation required reviewers.

**Tasks:**

1. Implement users, bootstrap admin, Argon2id credentials, opaque hashed
   sessions, CSRF defense, recovery-code issuance, and central authorization.
2. Implement `Library`, `Node`, `FileVersion`, `Object`, `ObjectReplica`,
   `StorageBackend`, and lease metadata with constraints and revision/ETag
   behavior.
3. Implement safe local `ObjectStore`: generated keys, bounded streaming,
   staging, durable promotion, head/read/range, abort, and controlled delete.
4. Implement folder/list/rename/move and a basic streamed content commit with
   transactionally coupled journal/audit/outbox.
5. Implement range download/content-disposition, quota prechecks, disk capacity
   reporting, and stable error mapping.
6. Build a minimal accessible web file manager over generated API types.

**Allowed parallel work:** Auth, logical metadata, local adapter, web UI and
adapter conformance tests after schema/API contracts freeze. Content commit
orchestration has one owner and merges after adapter and DB primitives.

**Dependencies:** PostgreSQL primary, selected local root identity, accepted
ObjectStore and logical-object contracts, central policy interface, Caddy
streaming configuration, and closure of OD-SYNC-004 before the Node schema/API
freeze if its representation mutates ancestor or subtree state.

**Expected files/components:** domain/application/auth/API crates;
metadata-Postgres migrations/repositories; object-store/local adapter; API
contract; web Files views; storage/deployment/runbook docs; integration tests.

**Tests:** Authorization matrix/IDOR; path traversal/symlink/TOCTOU; name
normalization/collision/cycle; SQL binding; CSRF/XSS filename; streaming/range;
zero/large file; quota/disk-full; durability; object-written/DB-failed;
DB-committed/response-lost; Compose E2E.

**Definition of done:** Authorized users can create/list/move/rename, upload and
range-download a verified file; files are never full-buffered; no visible
metadata can reference partial/missing bytes.

**Security gate:** Auth/session/recovery controls, object lookup scoping, path
safety, filename output safety, body/rate/concurrency limits, secret/log
redaction, and negative authorization tests pass.

**Performance gate:** Memory is bounded independently of object size; publish
reproducible listing, concurrent streaming, range, SHA-256 and DB-pool baselines
with accepted regression budgets.

**Documentation gate:** API, storage, auth/security, operator capacity/disk-full,
and web status docs updated in both languages.

**Next-phase gate:** Record `SV-G1-STORAGE`; freeze the basic object commit and
OD-SYNC-004 recursive-subtree precondition contracts before resumable
completion or recursive Trash work.

### Phase 2 — Reliable uploads and data safety

**Objective:** Make upload, version, trash, integrity, reconciliation and GC
safe across interruption, retry and crash.

**Prerequisite gate:** `SV-G1-STORAGE`; local adapter conformance green.

**Responsible workstreams:** Storage / Uploads accountable; Database / Jobs,
Backup, Security, QA, Rust Backend, Web, DevOps and Documentation responsible.

**Tasks:** Implement the persisted `UploadSession`/`UploadPart` state machine;
part/status/retry APIs; serialized assembly/verification/completion; persisted
terminal outcomes; version browsing/restore; trash/purge retention; orphan and
staging reconciliation; integrity scan/quarantine; leases; reference-aware
mark/sweep GC; logical/physical accounting and operator audit tools.

**Allowed parallel work:** Part transport, versions/trash UI, invariant auditor,
and failure-injection harness after lifecycle schemas freeze. Completion and GC
eligibility are serialized contract areas with one integrator.

**Dependencies:** Stable content commit, SHA-256/representation checksum rules,
job/outbox leases, backend durability behavior, every protected reference type,
and the accepted OD-SYNC-004 subtree precondition from `SV-G1-STORAGE`.

**Expected files/components:** uploads, storage, jobs, versions/trash domain/API,
migrations, reconciliation/integrity workers, web transfer/version/trash views,
failure harness and runbooks.

**Tests:** Part duplicate/different-payload conflict; gap/overlap/overflow;
expiry race; concurrent completion; lost completion response; crash at every
object/DB/outbox boundary; disk/quota exhaustion; corrupt object; active lease;
orphan grace; GC dry/live; recursive purge bounds; restore version hash.

**Definition of done:** One logical outcome per idempotency key; committed
objects are verified; orphan/staging leakage is observable and safely reclaimed;
version/trash restore works; GC cannot delete a protected reference.

**Security gate:** Session/part ownership, resource limits, checksum trust,
cleanup authorization, quarantine access and audit pass adversarial review.

**Performance gate:** Assembly/checksum concurrency is bounded; benchmark part
sizes, completion, integrity scan and GC on declared datasets; API latency is
not blocked by optional worker work.

**Documentation gate:** Upload state/errors/retry, version/trash retention,
integrity/quarantine, accounting, GC and crash runbooks aligned bilingually.

**Next-phase gate:** Record `SV-G2-DATA-SAFETY` only after invariant audit and
restore evidence; enable sharing/device and optional prototype branches.

### Phase 3 — Sharing and device foundation

**Objective:** Add auditable, revocable delegated access and device identities.

**Prerequisite gate:** `SV-G2-DATA-SAFETY`; ownership/isolation decision closed.

**Responsible workstreams:** Auth / Security accountable; Rust Backend,
Database, Devices/Clients, Web, QA, DevOps and Documentation responsible.

**Tasks:** Private shares; public link capabilities; expiry/password/permission;
atomic revoke; share audit; device registration; scoped credentials;
rotation/revocation; last-seen/status/pause; recovery; rate limiting and abuse
budgets; Security/Devices/Shared UI.

**Allowed parallel work:** Device and share domains after common policy/audit
contracts freeze. Public-share edge handling and authenticated share UI may run
in parallel. One policy owner integrates effective permission rules.

**Dependencies:** Central authorization, ownership boundary, opaque token/hash
model, audit schema, recursive-node authorization semantics, proxy/client IP
trust configuration.

**Expected files/components:** auth/sharing/devices domain/application/API;
migrations; rate-limit/audit components; web Shared/Devices/Security views;
security/operations docs and abuse tests.

**Tests:** Cross-user IDOR; inherited/recursive grants; writable/read-only;
expiry/time skew; optional password brute force; token hashing/non-enumeration;
revoke during download/mutation; rotation replay; device revoke; session epoch;
recovery takeover; rate dimensions; audit redaction.

**Definition of done:** Every grant is explicit and revocable; possession of an
ordinary ID gives no access; revoked credentials fail within the documented
bound; UI and audit report truthful state.

**Security gate:** Independent authorization matrix and public-abuse review,
credential/recovery threat tests, no raw token persistence/logging, and stable
revoke behavior.

**Performance gate:** Authorization and rate checks have bounded queries and no
unbounded subtree load; public download concurrency/bytes are budgeted.

**Documentation gate:** Sharing permissions/limits, device revoke limitations,
recovery and activity data documented in both languages.

**Next-phase gate:** Record `SV-G3-TRUSTED-ACCESS`; device identity and grants
are frozen inputs to sync.

### Phase 4 — Sync protocol

**Objective:** Deliver a reference-conformant ordered change feed and safe
offline mutation protocol.

**Prerequisite gate:** `SV-G3-TRUSTED-ACCESS`; all namespace mutations emit
reviewed journal facts.

**Responsible workstreams:** Sync accountable; Database, Rust Backend, Clients,
Storage, Security, QA, DevOps and Documentation responsible.

**Tasks:** Per-library clock/epoch; versioned cursor codec; ordered change pages;
initial snapshot/rebaseline; cursor retention; checkpoints; mutation IDs;
ETag/base versions; deterministic content conflict copies; metadata rebase;
delete/restore/move/cycle/name rules; reference state-machine client; versioned
fixtures and simulator.

**Allowed parallel work:** Server journal, reference client and model-based
fixtures after protocol freeze. UI conflict presentation may consume fixtures.
Clock allocation/cursor/conflict contract edits are single-writer.

**Dependencies:** Device credentials, logical metadata/version/trash semantics,
idempotency store, portable names, transactional journal invariant, operational
retention configuration.

**Expected files/components:** sync domain/application/API, migrations,
client/reference core, protocol fixtures, web Activity/conflict surfaces,
metrics/runbooks, `SYNC` specs and test harness.

**Tests:** Every scenario and invariant in the sync section of
[TESTING.md](TESTING.md), including offline/offline, delete/edit, duplicate/lost
response, writes during pagination, stale/foreign/old-epoch cursor, atomic local
page application, directory races, backlog, server restart, disk full and
revoked device.

**Definition of done:** Supported clients cannot silently miss a committed event
or silently overwrite conflicting bytes; expired history has a complete server-
directed rebaseline; fixture behavior matches server and reference client.

**Security gate:** Cursor integrity/scope, mutation authorization, revoked
device, replay/idempotency, metadata leakage and abuse/backlog limits reviewed.

**Performance gate:** Record per-library mutation contention, page/list latency,
change-feed lag, client apply cost and backlog catch-up under declared
concurrency; memory and page size remain bounded.

**Documentation gate:** Normative sync protocol, client obligations, retention,
conflict UX, errors and recovery aligned in English/Vietnamese and OpenAPI.

**Next-phase gate:** Record `SV-G4-SYNC-CONFORMANT`; freeze conformance fixtures
for backup/desktop consumers.

### Phase 5 — Backup and restore

**Objective:** Deliver independently retained, verifiable device snapshots and
non-destructive recovery.

**Prerequisite gate:** `SV-G4-SYNC-CONFORMANT`; `BackupSet`/snapshot contract
reviewed as separate from live sync.

**Responsible workstreams:** Backup / Restore accountable; Storage, Database,
Clients, Rust Backend, Security, QA, Web, DevOps and Documentation responsible.

**Tasks:** Backup set/policy/source definitions; building/verifying/committed
manifest state; incremental unchanged reuse; consistency labels; retention and
holds; device status; restartable restore operations; collision policies;
file/folder/snapshot recovery; clean-device workflow; operator recovery metrics.

**Allowed parallel work:** Client scanner, server snapshot builder, restore
planner and web UI after manifest format/API freeze. Retention and GC integration
merge only after all reference classes are modeled.

**Dependencies:** Verified objects, leases/GC, device identity, durable jobs,
quota/accounting, versioned manifest, restore destination policy.

**Expected files/components:** backup domain/application/API; migrations;
manifest codec; retention/restore workers; client backup adapter; web Backups/
Restore views; fixtures, runbooks and docs.

**Tests:** Every backup scenario in [TESTING.md](TESTING.md): local deletion,
omission, interrupted/repeated scan, unchanged/changed large files, concurrent
change, snapshot commit atomicity, corruption, quota, removed device,
retention/reference safety, partial restore/retry, clean-destination recovery.

**Definition of done:** Only complete verified snapshots are restorable; missing
source never propagates live deletion; retention preserves every selected
snapshot; independent clean restore verifies expected bytes and reports skips.

**Security gate:** Backup set ownership, manifest/path handling, restore target
authorization, symlink policy, protected-history deletion, device loss and
audit reviewed.

**Performance gate:** Manifest memory/page bounds, unchanged-file efficiency,
snapshot commit, retention scan and restore throughput measured with small-file
and large-file profiles.

**Documentation gate:** Backup versus sync, consistency labels, schedules,
retention, restore collision/error/report and device-loss runbook aligned.

**Next-phase gate:** Record `SV-G5-RESTORABLE`; a separate reviewer signs the
clean restore evidence.

### Phase 6 — Storage optimization

**Objective:** Add measured compression and bounded whole-object dedup without
changing logical data or lifecycle guarantees.

**Prerequisite gate:** `SV-G5-RESTORABLE`; all protected references known to GC.

**Responsible workstreams:** Storage accountable; Database, Backup, Security,
QA, DevOps, Rust Backend and Documentation responsible.

**Tasks:** Compression classifier/policy; Zstandard representation metadata;
plaintext/stored checksums; decode path; dedup lookup/verification within domain;
logical/physical accounting; re-encoding/copy-switch jobs; GC/reference audit;
feature flags and compatibility readers.

**Allowed parallel work:** Benchmarks/classifier, codec adapter and accounting
UI after representation contract freeze. Dedup aliasing and GC remain serially
integrated.

**Dependencies:** Immutable object model, SHA-256, versioned representation,
backup references, robust restore, storage migration capability.

**Expected files/components:** object-store codecs/policy, metadata/migrations,
workers, accounting queries/UI, adapter conformance, benchmark/fuzz corpora,
storage/upgrade docs.

**Tests:** Byte-exact round trip; eligible/ineligible types; truncated/corrupt
frame; decompression bomb; codec version; hash collision quarantine path;
same/different owner; concurrent dedup; retention/GC; old representation reads;
interrupted migration and rollback window.

**Definition of done:** Enabled policies measurably save storage on declared
workloads; disabling them preserves reads/restores; no cross-owner existence
leak or premature delete exists.

**Security gate:** Decompression limits, cross-domain side channels,
authorization/accounting, parser library/supply-chain and migration secret/key
impact reviewed.

**Performance gate:** Publish compression ratio, CPU, throughput, memory, range-
read amplification and dedup lookup/GC overhead by workload; only beneficial
rules enable by default.

**Documentation gate:** Codec/hash/accounting semantics, skipped formats,
migration/rollback and privacy boundaries aligned.

**Next-phase gate:** Record `SV-G6-OPTIMIZED-SAFELY`; advanced `PLANNED` chunk
dedup stays behind a new accepted ADR and later measured promotion gate.

### Phase 7 — Desktop reference client

**Objective:** Prove selected-folder sync and backup on real desktop filesystem
semantics.

**Prerequisite gate:** `SV-G4-SYNC-CONFORMANT`; backup claims also require
`SV-G5-RESTORABLE`.

**Responsible workstreams:** Clients accountable; Sync, Backup, Security, QA,
Release, Documentation and platform owners responsible.

**Tasks:** Decide shared Rust core boundary; durable local state; onboarding;
device credentials; initial/rebaseline/incremental sync; filesystem watcher plus
rescan; conflict/status UX; selected backup; bandwidth/retry; packaging/update
policy and diagnostics.

**Allowed parallel work:** Protocol core and individual platform adapters after
fixture freeze. One local-state schema owner and one packaging owner integrate.

**Dependencies:** Sync fixtures, portable names, device credentials, range/
resumable endpoints, backup contract, OS key stores and supported-platform
matrix.

**Expected files/components:** real `clients/sync-core` and platform package only
when implemented; platform adapter tests; packaging/release assets; client docs
and support matrix.

**Tests:** Reference fixtures plus case/Unicode/reserved names, symlink/loops,
locked file, permissions, watcher overflow, clock skew, atomic replace, reboot/
crash, local DB corruption, low disk, network changes, conflict, stale cursor,
credential revoke, full rebuild and uninstall/cache semantics.

**Definition of done:** Each claimed platform completes initial and incremental
sync, offline conflict preservation, device revoke and restore/rebaseline under
published limits.

**Security gate:** Key-store use, update/package integrity, local DB/path
permissions, token/path log redaction, symlink handling and diagnostics consent.

**Performance gate:** Initial scan, watcher/reconcile, local DB size, CPU/memory,
network concurrency and backlog catch-up measured on declared directory shapes.

**Documentation gate:** Installation, policy, conflicts, status, supported
filesystems/platforms, limits and troubleshooting aligned.

**Next-phase gate:** Record `SV-G7-DESKTOP-REFERENCE`; use client evidence for
Apple and smart-storage decisions.

### Phase 8 — Photos

**Objective:** Build an original-preserving photo library with disposable,
privacy-controlled derivatives.

**Prerequisite gate:** `SV-G5-RESTORABLE`; photo deletion/backup policy selected
before promotion.

**Responsible workstreams:** Photos accountable; Storage, Backup, Security, Web,
QA, Clients, DevOps and Documentation responsible.

**Tasks:** Photo asset/group model; original upload/import idempotency; timeline,
album/favorite metadata; EXIF extraction; thumbnail/rendition workers; image/
video/screenshot classification; exact duplicates and suggestion provenance;
search metadata; derivative purge/rebuild; backup status.

**Allowed parallel work:** UI/timeline, parser sandbox, derivative pipeline and
PhotoKit contract after photo schema/event freeze. Original lifecycle has one
storage/backup integrator.

**Dependencies:** Canonical upload/version/backup, job leases, parser isolation,
privacy policy, supported codec/runtime inventory.

**Expected files/components:** photos domain/API/jobs, migrations, web Photos
views, parser/rendition service/container policy, corpora/fixtures and docs.

**Tests:** Malformed/truncated/huge-dimension media; image/video limits;
unsupported HEIC/HEVC preview; EXIF location ACL/share stripping; original hash;
derivative stale/rebuild/delete; duplicate suggestions; grouped resources;
source deletion; retention/restore; worker absent/crash.

**Definition of done:** Originals remain intact/downloadable/restorable without
photo workers; derivatives are version-bound/rebuildable; sensitive metadata is
authorized; source-deletion behavior is explicit.

**Security gate:** Parser sandbox/egress/resource limits, EXIF/location privacy,
malicious SVG/content sniffing, sharing and derivative cache authorization.

**Performance gate:** Timeline pagination, thumbnail queue age, pixel/CPU/memory
budgets, video upload and derivative storage amplification measured.

**Documentation gate:** Original/derivative semantics, privacy, codecs, delete/
backup behavior and worker degradation aligned.

**Next-phase gate:** Record `SV-G8-PHOTOS-SAFE`; freeze PhotoKit-facing API and
group semantics.

### Phase 9 — Apple client contract prototype

**Objective:** Validate the Apple platform boundary and produce disposable
contract/fixture evidence. This is the early prototype step from the master
plan's Apple workstream, not final Apple-client promotion; supported Apple
workflows remain gated after Forgejo/code integration.

**Prerequisite gate:** `SV-G7-DESKTOP-REFERENCE` and `SV-G8-PHOTOS-SAFE`; shared
core prototype decision recorded.

**Responsible workstreams:** Apple Clients accountable; Sync, Photos, Backup,
Security, QA, Release and Documentation responsible.

**Tasks:** Swift/SwiftUI onboarding/status; Keychain auth; background URLSession
upload; PhotoKit incremental import/limited permission; FileProvider enumeration,
change anchors, hydrate/evict; device policy; conflict/error UI; packaging and
privacy declarations.

**Allowed parallel work:** PhotoKit and FileProvider adapters after server
contracts freeze; auth/onboarding may run independently. One Apple release
integrator owns entitlements and package.

**Dependencies:** Server capability negotiation, cursor/resumable/range APIs,
photo group/import IDs, backup policy, real devices/OS matrix, App Store license
review.

**Expected files/components:** Swift packages/apps only when real work begins;
generated API layer; platform fixtures; CI/device test plan; privacy/support
docs.

**Tests:** Limited/full/no PhotoKit permission; permission revoke; duplicate
callback; edited/deleted asset; background termination/resume; low power/data/
disk; FileProvider stale anchor and placeholder states; conflict; Keychain/
device revoke; server compatibility.

**Definition of done:** The prototype and platform fixtures exercise the claimed
boundary, respect permission scope, and truthfully describe background/backup
limits without claiming a supported product client.

**Security gate:** Keychain, ATS/TLS trust, URL/open redirect handling,
entitlements, local cache protection, diagnostics/privacy labels and remote
cache-delete wording reviewed.

**Performance gate:** Background transfer, enumeration page/apply, hydration,
battery/network and local-cache behavior measured on supported real devices.

**Documentation gate:** Onboarding, permissions, backup versus sync, FileProvider
states, troubleshooting and non-goals aligned.

**Next-phase evidence:** Record `SV-G9-APPLE-CONTRACT-PROTOTYPE` as an internal
prototype evidence bundle only. The canonical `SYNVEIL_APPLE_CLIENT_READY` gate
cannot be promoted until the master-sequence Apple promotion step after
`SYNVEIL_CODE_INTEGRATION_READY`.

### Phase 10 — AI foundation

**Objective:** Add optional derived intelligence with no silent remote data
egress and no core availability dependency.

**Prerequisite gate:** `SV-G2-DATA-SAFETY`; durable job/outbox, authorization and
derived-record lifecycle contracts stable.

**Responsible workstreams:** AI / Privacy accountable; Security, Rust Backend,
Database, DevOps, Search/Web, QA, Documentation and model-license owner
responsible.

**Tasks:** Mode/config policy; Python runtime boundary; scoped job inputs; OCR/
text extraction; embedding/vector records; metadata/semantic search blend;
AI-tag provenance; index freshness; retry/dead-letter; per-item exclusion;
remote-provider disclosure/credentials; purge/rebuild and model migrations.

**Allowed parallel work:** Local model benchmarks, Python sandbox, derived schema
and search UI after data classification and job contract freeze. Remote mode is
separate and cannot ride along implicitly.

**Dependencies:** Canonical version IDs/events, ACL queries, optional pgvector
deployment, parser sandbox, provider/model/asset licenses, resource profile.

**Expected files/components:** `services/ai`, AI domain/jobs/API, optional
migrations/extension profile, web Search/AI settings, model metadata, privacy/
operations docs and adversarial corpora.

**Tests:** Core with AI absent/offline; no-egress disabled/local; consent and
revocation remote; stale version; delete/revoke/purge; ACL change; prompt
injection; malicious documents; timeout/OOM/retry; dead-letter; reindex; provider
failure/rate limit; log/content redaction.

**Definition of done:** Core features and deterministic search work without AI;
mode and freshness are visible; remote data categories require effective
policy; derived records are rebuildable and purgeable.

**Security gate:** Data-flow/privacy review, exact provider/origin allowlist,
least-privilege content capability, network/parser/resource sandbox, secret
storage, ACL-at-query and deletion lag pass.

**Performance gate:** Publish model/hardware profiles, queue age, throughput,
memory/CPU/GPU, index size/query latency and backpressure; AI cannot starve API/
database/storage critical work.

**Documentation gate:** Disabled/local/remote behavior, data categories,
provider retention, model/license, exclusions, freshness and deletion aligned.

**Next-phase gate:** Record `SV-G10-AI-OPTIONAL`; remote mode may remain disabled
even if local mode promotes.

### Phase 11 — Forgejo integration

**Objective:** Discover and restore Forgejo repositories without implementing a
forge or making Forgejo a core dependency.

**Prerequisite gate:** `SV-G5-RESTORABLE`; connector credential/SSRF and backup
representation decisions reviewed.

**Responsible workstreams:** Integrations accountable; Backup, Security, Rust
Worker, Database, Web, QA, DevOps and Documentation responsible.

**Tasks:** Connector interface; Forgejo compatibility adapter; encrypted secret
storage/rotation; exact-origin policy; polling and webhook hint; repository/ref/
commit inventory; staleness/health; Project association; Git/LFS/artifact backup
manifest; verification and non-destructive restore.

**Allowed parallel work:** Inventory UI, connector adapter and backup-format
fixture work after contract freeze. Credential/SSRF layer and restore execution
have single owners.

**Dependencies:** Durable jobs, backup objects/manifests, master-secret recovery,
Forgejo test matrix, Git tooling/process sandbox, project/ownership rules.

**Expected files/components:** integrations connector/Forgejo modules, jobs/API,
migrations, web Code/Projects/Settings, compatibility lab fixtures and runbooks.

**Tests:** Private/local origin opt-in, DNS rebinding/redirect, credential
redaction/rotation, webhook HMAC/replay/duplicate/size, polling reconciliation,
outage/stale state, repository/LFS/artifact coverage, `git fsck`, interrupted
backup, clean restore, version incompatibility, overwrite refusal.

**Definition of done:** Forgejo failure degrades only integration surfaces;
claimed backup coverage is manifest-explicit and independently restorable;
Synveil exposes no custom Git smart HTTP/SSH.

**Security gate:** Least-scope encrypted credentials, egress/SSRF, webhook,
process execution, repository path/content, restore target and audit reviewed.

**Performance gate:** Poll/webhook coalescing, rate-limit backoff, large repo/LFS
memory/disk/throughput and queue fairness measured; core worker jobs are not
starved.

**Documentation gate:** Compatibility, scopes, backup coverage/omissions,
staleness, restore safety and outage behavior aligned.

**Next-phase gate:** Record `SV-G11-FORGEJO-RESTORABLE`; repository intelligence
waits for both AI and connector gates.

### Phase 12 — AI plus code

**Objective:** Provide ACL-correct, provenance-bearing repository search and
question answering over versioned source.

**Prerequisite gate:** `SV-G10-AI-OPTIONAL` and
`SV-G11-FORGEJO-RESTORABLE`.

**Responsible workstreams:** AI / Privacy and Integrations jointly accountable;
Security, Web, QA, DevOps and Documentation required reviewers.

**Tasks:** Commit/ref-bound extraction; file/language/generated/binary policies;
README/docs/commit metadata indexing; semantic code search; project retrieval;
answer citations/provenance; refresh/delete; optional issue/PR adapter with
permission mapping; prompt-injection and secret exclusion.

**Allowed parallel work:** Indexer and query UI against fixed ACL/provenance
fixtures. One retrieval/authorization owner integrates final result filtering.

**Dependencies:** Current connector inventory and access relationships,
version-bound AI records, deletion, model context/size limits, project links.

**Expected files/components:** code indexer/AI adapters, search APIs/UI, jobs,
fixtures/corpora, privacy/security/operations docs.

**Tests:** Cross-user/project/repo ACL; revoked access; stale branch/ref;
repository delete; secret/generated/binary exclusion; prompt/tool injection;
malicious/huge repository; remote consent; citation correctness; provider
failure and complete reindex.

**Definition of done:** Results cite indexed versions, never cross current ACLs,
label staleness, and disappear/rebuild under lifecycle policy; no AI output is
treated as repository truth.

**Security gate:** Independent authorization and prompt-injection review,
secret scanning/exclusion, provider data classification and content-derived URL/
command non-execution.

**Performance gate:** Incremental index cost, context limits, queue fairness,
vector/filter latency and index size measured on declared repository shapes.

**Documentation gate:** Indexed/omitted sources, permissions, freshness,
provider modes, limitations and non-authoritative answers aligned.

**Next-phase gate:** Record `SV-G12-CODE-INTELLIGENCE`; no expansion to forge
protocols or autonomous code mutation.

### Phase 12B — Apple client promotion in the master sequence

**Objective:** Promote the Apple client only after the master plan's preceding
client, Photos, AI, and Forgejo/code contracts are stable.

**Prerequisite gate:** `SYNVEIL_DESKTOP_SYNC_READY`,
`SYNVEIL_PHOTOS_FOUNDATION_READY`, `SYNVEIL_CODE_INTEGRATION_READY`, and
`SYNVEIL_CLIENT_CONTRACT_READY`. The Phase 9 Apple prototype may supply
fixtures, but it is not a substitute for these gates.

**Responsible workstreams:** Apple Clients accountable; Sync, Uploads, Photos,
Backup, Security, QA, Release and Documentation responsible.

**Tasks:** Reconcile the prototype with the final versioned server contracts;
complete device auth/revocation, PhotoKit import and permission behavior,
FileProvider enumeration/change anchors/hydration where supported, background
URLSession resume, conflict/error UX, package/entitlement review, and the
support matrix for each claimed OS and device.

**Tests:** Run the full platform matrix for limited/full/revoked PhotoKit
permission, duplicate callbacks, edited/deleted assets, background termination
and resume, low power/network/storage, stale FileProvider anchors, conflict,
Keychain/device revoke, server upgrade compatibility, clean restore, and
documented best-effort scheduling. A required unsupported capability is an
explicit error/state, never a silent simulation.

**Definition of done:** The supported Apple workflows recover from interruption,
respect permission and authorization scope, interoperate with the frozen
server/client fixtures, and accurately state backup/background limitations.

**Security and operations gate:** Keychain/ATS/TLS, entitlements, local cache,
privacy declarations, diagnostics, release provenance, support runbook, and
rollback/update behavior are independently reviewed.

**Next-phase gate:** Promote the exact canonical token
`SYNVEIL_APPLE_CLIENT_READY`; do not claim full-device backup or guaranteed
continuous background execution.

### Phase 13 — Smart storage

**Objective:** Add files-on-demand, deterministic tiering and recoverable anomaly
safeguards without losing a verified copy.

**Prerequisite gate:** `SYNVEIL_STORAGE_OPTIMIZED` (the expanded roadmap's
`SV-G6-OPTIMIZED-SAFELY` evidence), `SYNVEIL_APPLE_CLIENT_READY`, a conformant
capable client, and accepted placement/state contracts. Isolated prototypes
may run earlier only in a disposable scope and cannot promote the phase.

**Responsible workstreams:** Storage and Clients jointly accountable; Sync,
Backup, Security, QA, DevOps, Web and Documentation responsible.

**Tasks:** Placeholder/local state model; hydrate/range/resume; verified cache
eviction; pin/exclude policy; replica tier rules; copy/verify/switch/retire jobs;
capacity recommendations; destructive-burst heuristics, pause/review/override;
recovery UX and metrics.

**Allowed parallel work:** Server placement engine and each client placeholder
adapter after state-machine fixture freeze. Replica retirement has one storage/
backup integrator.

**Dependencies:** Object replicas/leases, storage migration, sync state,
supported client capabilities, restore, backend health/capacity, policy audit.

**Expected files/components:** placement policy/domain/jobs, client adapters,
API/UI status, migrations if needed, state fixtures, operations and recovery
docs.

**Tests:** Hydrate/evict/pin races; partial range; low disk; offline/backend
failure; interrupted replica move; target corruption; rollback window; stale
client; delete versus eviction; rule disable; anomaly false positive/override;
restore from each tier.

**Definition of done:** At least one verified reachable copy remains; cache
state never masquerades as server deletion; rule decisions are explainable and
reversible; anomaly handling cannot permanently trap legitimate operations.

**Security gate:** Local placeholder/cache permissions, policy authorization,
backend credentials, archive restore, anomaly abuse/lockout and audit reviewed.

**Performance gate:** Hydration latency/throughput, range amplification, local
cache/accounting, placement copy cost, backend queue and rule evaluation
measured; core operations retain priority.

**Documentation gate:** State meanings, platform support, policy explanations,
recovery/override and anomaly limitations aligned.

**Next-phase gate:** Record `SV-G13-SMART-STORAGE`; advanced scale work still
requires independent measured triggers.

### Phase 14 — Advanced scale

**Objective:** Address a measured bottleneck or availability goal with the
smallest compatible topology change.

**Prerequisite gate:** `SV-G13-SMART-STORAGE` plus
`SYNVEIL_APPLE_CLIENT_READY` for the master-sequence product promotion, or a
capability-specific measured trigger plus accepted ADR for earlier isolated
S3/worker scale work.

**Responsible workstreams:** Architecture and DevOps / Release accountable;
affected Database, Storage, Sync, Security, QA, Operations and Documentation
workstreams responsible.

**Tasks:** Measure and identify trigger; test simpler tuning; write ADR;
implement only the justified adapter/replica/broker/read-replica/Kubernetes
change; mixed-version/rollback plan; partition/load/recovery/security tests;
support runbook and cost/capacity model.

**Allowed parallel work:** Adapter or load harness prototypes may run against
frozen contracts. Multiple consistency/topology writers are not allowed.

**Dependencies:** Production measurements, support ownership, shared storage
semantics, job/journal coordination, rate/session consistency, backup/restore
for the new topology.

**Expected files/components:** Superseding ADRs, adapter/topology code, deploy
profiles/manifests, test lab, migration/rollback, operational dashboards and
docs. Kubernetes artifacts are absent unless this gate specifically approves
them.

**Tests:** ObjectStore conformance; API/worker replica races; journal ordering;
job lease generation; shared rate limits/session revocation; network partition;
read replica lag; broker duplicate/loss; node loss; upgrade/rollback;
coordinated backup/restore and declared load envelope.

**Definition of done:** The measured goal improves without weakening earlier
invariants, and operators can deploy, observe, upgrade, restore and roll back
the topology within documented limits.

**Security gate:** New trust boundary, network identity/TLS, secrets/RBAC,
multi-node authorization/cache lag, supply chain and incident response reviewed.

**Performance gate:** Before/after results on identical methodology prove the
trigger is addressed; cost and tail-latency/regression trade-offs are accepted.

**Documentation gate:** ADR, topology, capacity, health, failure, upgrade,
rollback, support and bilingual architecture docs aligned.

**Next-phase gate:** Record capability-specific `SV-G14-SCALE-PROVEN`; do not
declare general “distributed readiness” beyond tested topology.

## Gate evidence bundle

The phase integration owner publishes one reviewable bundle containing:

- accepted contract/ADR revisions and closed blocking decisions;
- exact source/config/migration/format version;
- CI and environment matrix results;
- failure/recovery, security and performance reports;
- known defects with severity and release disposition;
- deployment/upgrade/rollback/restore evidence;
- English/Vietnamese documentation parity review;
- status changes requested and the next permitted task set.

Evidence is versioned with the release or linked immutably. Screenshots and
claims without commands/fixtures/configuration are supplementary, not proof.

## Program risk handling

Each risk has an accountable workstream, trigger, mitigation, verification and
release consequence. Minimum standing risks are sync omission/overwrite,
metadata/object divergence, backup/sync confusion, premature GC, migration
failure, cross-user authorization, compromised device, parser compromise,
remote AI egress, Git credential/SSRF, mobile background limits, license
ambiguity and scope expansion.

If a trigger indicates possible data loss or unauthorized disclosure:

1. stop feature promotion and destructive automation;
2. preserve logs/audit, affected object/snapshot references and reproduction
   evidence without copying sensitive contents unnecessarily;
3. classify blast radius and whether canonical data remains verified;
4. create a bounded corrective task and independent recovery/security review;
5. update runbooks and regression fixtures before resuming the gate.

## How future agents consume this plan

The coordinator selects exactly one not-yet-satisfied work package under the
current phase, copies the standard task contract, fills concrete paths and
commands from the current repository, pins prerequisite revisions, and names a
reviewer. The agent returns the required report. The coordinator does not issue
the next dependent task until evidence is integrated and the named gate—not
merely the agent's confidence—passes.

## Master-plan workstream map and mandatory gates

The master execution plan names ten functional workstreams plus Team Q as the
cross-cutting reliability role. This document further splits some of those
teams into specialist workstreams so that ownership is actionable; the split
does not create a second authority or permit a specialist to redefine a
contract owned by its master team.

| Master team | Detailed workstreams in this plan | Canonical responsibility | Master gate or release gate |
|---|---|---|---|
| Team A — Architecture & Contracts | Architecture / Contracts, Documentation | ADRs, `docs/`, `api/openapi.yaml`, domain/protocol contracts, errors, versioning, cross-team decisions | `SYNVEIL_CONTRACTS_READY` |
| Team B — Core Backend & API | Rust Backend, Database / Jobs | Rust workspace, Axum API, application services, PostgreSQL access, migrations, jobs, health | `SYNVEIL_API_FOUNDATION_READY` |
| Team C — Storage & Data Integrity | Storage / Uploads | ObjectStore, staging, uploads, versions, integrity, compression, dedup, GC | `SYNVEIL_STORAGE_FOUNDATION_READY`, `SYNVEIL_UPLOADS_RELIABLE`, `SYNVEIL_DATA_INTEGRITY_READY` |
| Team D — Sync Engine | Sync | Change journal, cursors, device checkpoints, conflict behavior, sync conformance | `SYNVEIL_SYNC_PROTOCOL_STABLE` |
| Team E — Backup & Recovery | Backup / Restore | Backup sets, manifests, snapshots, retention, restore, disaster-recovery evidence | `SYNVEIL_BACKUP_RESTORE_READY` |
| Team F — Web Application | Web | React/TypeScript/Vite application, generated client, transfer/recovery/conflict UX | `SYNVEIL_WEB_CORE_READY` |
| Team G — Security & Identity | Auth / Security | Passwords, sessions, device credentials, authorization, shares, abuse controls, threat model | `SECURITY_REVIEW_PASS` |
| Team H — Infrastructure & DevOps | DevOps / Release | Compose/Caddy, images, CI, configuration, observability, release and upgrade runbooks | `SYNVEIL_DEPLOYMENT_READY` |
| Team I — AI & Search | AI / Privacy | Optional Python worker, OCR, extraction, embeddings, search, provenance, privacy modes | `SYNVEIL_AI_FOUNDATION_READY` |
| Team J — Integrations & Clients | Integrations, Clients, Photos | Forgejo boundary, desktop/Apple clients, photo subsystem and platform contracts | `SYNVEIL_CLIENT_CONTRACT_READY` |
| Team Q — QA & Reliability | QA / Reliability | Integration, protocol, E2E, property/fuzz, failure injection, recovery and benchmarks | Independent review of every gate |

### Team A — Architecture & Contracts

Team A owns `docs/`, `api/openapi.yaml`, ADRs, domain contracts,
cross-module interfaces, the error model, versioning rules, and dependency
resolution between teams. Its critical outputs are `ARCHITECTURE.md`,
`DOMAIN_MODEL.md`, `API_ARCHITECTURE.md`, `STORAGE.md`, `SYNC.md`,
`BACKUP.md`, `SECURITY.md`, `PLATFORM.md`, the reviewed OpenAPI artifact, and
the ADR index. Platform / Distribution ownership includes the two deployment
modes, `PlatformRuntime`/service lifecycle, storage capabilities, pairing,
remote-access layers, health translation, and progressive-disclosure rules. It
does not authorize a native package or hosted relay by documentation alone.
It maintains the architecture and reviews changes to IDs, object format,
sync semantics, API conventions, errors, authentication, and database
ownership. It may implement shared types, generated contracts, contract
validation, or scaffolding, but it does not implement the entire backend.
`SYNVEIL_CONTRACTS_READY` requires reviewed contracts and fixtures; it does not
authorize downstream teams to invent a parallel protocol.

### Team B — Core Backend & API

The baseline stack is Rust, Axum, Tokio, Tower, Serde, SQLx, and `tracing`.
Team B owns `crates/api`, `crates/core`, `crates/metadata`, server bootstrap,
HTTP middleware, routing, application services, transaction orchestration,
pagination, configuration, versioning, and health endpoints. Handlers follow:

```text
HTTP → validation → application service → domain/storage/database
     → typed result → HTTP mapping
```

Handlers remain thin, typed, testable, and observable. AI inference,
filesystem implementation details, sync conflict policy, and backup
retention policy do not belong in handlers. `SYNVEIL_API_FOUNDATION_READY`
requires typed error mapping, authorization hooks, transaction boundaries,
health behavior, and contract tests.

### Team C — Storage & Data Integrity

Team C owns `crates/storage`, `crates/object-store`, `crates/uploads`,
`crates/versions`, `crates/compression`, and `crates/dedup`, including the
`ObjectStore` port, local and later S3-compatible adapters, object IDs and
keys, safe writes, integrity verification, resumable uploads, checksums,
immutable versions, restore, staging cleanup, accounting, compression,
whole-object deduplication, garbage collection, storage picker integration,
filesystem capability probing, and portable fallback behavior. The golden rule
is:

```text
never publish metadata for bytes that are not durable and verified;
never leave an orphan or uncertain reference without a repair path.
```

At minimum, failure evidence covers disk full, process crash, checksum
mismatch, duplicate completion, lost HTTP response, temporary-file residue,
database transaction failure, object-write failure, and filesystem permission
failure. Every destructive maintenance path supports inspection and dry-run
where practical. The three Team C gates are separate evidence decisions:
`SYNVEIL_STORAGE_FOUNDATION_READY` for the adapter and durable object boundary,
`SYNVEIL_UPLOADS_RELIABLE` for resumable completion, and
`SYNVEIL_DATA_INTEGRITY_READY` for reconciliation, verification, and protected
references.

### Team D — Sync Engine

Team D owns `crates/sync`, the change journal, cursors, conflict engine,
protocol, and device synchronization state. Full implementation waits for
`SYNVEIL_DATA_INTEGRITY_READY`. Every sync-relevant create, update, rename,
move, delete, and restore produces a durable committed fact. Pull must recover
from stale cursors; push must be conditional and idempotent; a lost response
must not create a duplicate mutation. The conformance matrix covers create,
rename, move, delete, restore, concurrent edits, stale cursor, duplicate
request, server restart, reconnect, and at least 10,000 queued changes.
Promotion requires `SYNVEIL_SYNC_PROTOCOL_STABLE`.

### Team E — Backup & Recovery

Team E owns `crates/backup`, backup sets, sources, snapshots, manifests,
retention, restore workflows, backup verification, and backup health. It preserves the invariant
that synchronization and backup are different products:

```text
sync deletion may propagate current-state deletion;
backup deletion keeps historical snapshots until retention permits expiry.
```

Required evidence covers full and incremental backup, unchanged and modified
files, deleted input, a disappeared device, single-file and folder restore,
snapshot restore, and a clean instance-disaster recovery drill. A production
backup claim is forbidden until restore has been independently verified.
Promotion requires `SYNVEIL_BACKUP_RESTORE_READY`.

### Team F — Web Application

The web stack is React, TypeScript, and Vite under `apps/web`. Initial screens
cover login, dashboard, files, uploads, trash, versions, sharing, devices,
backups, and settings; Photos, Code, Projects, and AI Search follow their
respective gates. The frontend consumes generated/typed API contracts and does
not own canonical business logic, object keys, conflict resolution, or backup
semantics. It renders server-defined state, including `loading`, `empty`,
`success`, `partial`, `offline`, `error`, `conflict`, `uploading`, `paused`,
and `retrying`. UX / Accessibility owns plain-language user errors, keyboard/
screen-reader behavior, progressive disclosure, no-terminal core flows,
storage selection, pairing, health, update, uninstall, migration, and recovery
surfaces. `SYNVEIL_WEB_CORE_READY` requires accessible recovery and conflict UX,
typed API consumption, and failure-state tests.

### Team G — Security & Identity

Team G runs from the first phase and owns password hashing, login, session and
token lifecycle, device credentials, revocation, authorization, public-share
security, rate limits, audit logs, and the threat model. It reviews upload and
download authorization, object access, shares, Git credentials, AI-provider
credentials, webhooks, filesystem paths, and container secrets. Required
attack classes include IDOR, path traversal, XSS, CSRF where applicable, SQL
injection, SSRF, oversized upload, decompression bomb, malicious filenames,
token replay, and brute force. Each major release needs
`SECURITY_REVIEW_PASS`; a feature cannot bypass this gate because it is
“internal” or asynchronous.

### Team H — Infrastructure & DevOps

Team H owns `deploy/`, Dockerfiles, Compose profiles, Caddy, CI, release
pipeline, observability plumbing, native installer/update architecture, service
adapters, and the developer environment. Compose/Caddy is the Advanced / Server
reference topology; Personal / Home requires a separately evidenced native or
guided path. Installer / Updater owns signed artifact verification, preflight,
service recovery, managed-PostgreSQL coordination, data-preserving uninstall,
reinstall, migration, and release channels. Networking / Connectivity owns
pairing transport, proxy/TLS, LAN/remote layers, optional relay boundaries,
air-gapped behavior, and connectivity diagnostics. A new developer must have a
documented path to a bounded local environment, normally `docker compose up -d`
once implementation exists, but this is not an end-user support claim. CI must
cover Rust formatting, linting, tests, frontend lint/typecheck/tests, migration
validation, integration tests, docs links, secrets, dependencies, images,
installer fixtures, service lifecycle, and real-OS release-lab cases.
Production evidence includes graceful shutdown, health/readiness, DB and
storage backup, mount validation, logs, upgrades, signed artifacts, secrets,
service privileges, uninstall/migration preservation, and container permissions.
Promotion requires `SYNVEIL_DEPLOYMENT_READY` plus the platform evidence required
by `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY`.

### Team I — AI & Search

Team I owns `services/ai`, the optional Python runtime, OCR, text extraction,
embeddings, semantic search, AI tagging, and AI indexing. AI is always asynchronous:

```text
canonical file commit → durable event/job → AI worker → replaceable derived data
```

The initial feature sequence is OCR/text extraction/embeddings/document
search, then photo embeddings/tags/photo search, then code indexing and
project-aware search. Modes are `DISABLED`, `LOCAL`, and explicit `REMOTE`;
remote use is never silently enabled. Core file, sync, backup, download, and
restore remain available when AI is absent. `SYNVEIL_AI_FOUNDATION_READY`
requires no-egress evidence for disabled/local modes, ACL-safe derived data,
deletion/rebuild behavior, resource limits, and provider/privacy review.

### Team J — Integrations & Clients

Team J owns contracts beyond the core server. Stage 1 is the
`crates/integrations/forgejo` boundary: Forgejo integration connects to Forgejo,
inventories repositories, reports activity and health, captures
repository/Git-LFS backup, and associates projects; it never rewrites Git or
turns Forgejo outage into a core storage outage. Stage 2 targets desktop Linux,
Windows, and macOS and should reuse a shared Rust sync core, with Linux Server
used for host/service evidence rather than pretending it is a desktop shell.
Stage 3 future mobile work covers Android, iPhone, and iPad using their native
background, filesystem, notification, credential-store, and photo-library
boundaries; Apple work uses Swift, SwiftUI, FileProvider, PhotoKit, URLSession,
and Keychain within OS limits. Desktop / Mobile owns pairing, device lifecycle,
sync/backup parity, capability negotiation, and truthful unsupported states.
Large client implementation waits for `SYNVEIL_CLIENT_CONTRACT_READY`, which
freezes credentials, capabilities, sync/upload/backup behavior, and platform
fixtures.

### Team Q — QA & Reliability

Team Q owns `tests/integration`, `tests/e2e`, integration and protocol tests,
reliability scenarios, fault injection, and performance benchmarks, but every domain owner remains accountable for
the behavior under test. The test pyramid is:

```text
unit → integration → protocol → E2E → failure/recovery
```

Data loss, silent corruption, incorrect conflict resolution, failed restore,
incorrect delete propagation, and permission bypass take priority over UI
polish. QA also owns the real-OS matrix for Windows, macOS, Linux Desktop, and
Linux Server; install/storage-picker, service crash/reboot/sleep, signed update,
uninstall-with-data-preservation, managed database, pairing/remote-connectivity,
accessibility, and machine-migration fixtures. A skipped required scenario
keeps the task `PARTIAL`, not complete.

## Canonical mandatory gates

The following tokens are the exact master-plan promotion gates. They are the
only tokens that may be used to authorize the next major phase or to promote a
capability. A gate is an evidence decision recorded by its owner and reviewer,
not an agent's confidence statement. In this repository all are future gates;
the blueprint itself does not mark any one as passed.

```text
SYNVEIL_BLUEPRINT_COMPLETE
SYNVEIL_CONTRACTS_READY
SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY
SYNVEIL_FOUNDATION_READY
SYNVEIL_FILE_STORAGE_READY
SYNVEIL_UPLOADS_RELIABLE
SYNVEIL_DATA_LIFECYCLE_READY
SYNVEIL_SYNC_PROTOCOL_STABLE
SYNVEIL_BACKUP_RESTORE_READY
SYNVEIL_STORAGE_OPTIMIZED
SYNVEIL_DESKTOP_SYNC_READY
SYNVEIL_PHOTOS_FOUNDATION_READY
SYNVEIL_AI_SEARCH_READY
SYNVEIL_CODE_INTEGRATION_READY
SYNVEIL_APPLE_CLIENT_READY
```

The workstream gates `SYNVEIL_API_FOUNDATION_READY`,
`SYNVEIL_STORAGE_FOUNDATION_READY`, `SYNVEIL_DATA_INTEGRITY_READY`,
`SYNVEIL_WEB_CORE_READY`, `SECURITY_REVIEW_PASS`,
`SYNVEIL_DEPLOYMENT_READY`, `SYNVEIL_AI_FOUNDATION_READY`, and
`SYNVEIL_CLIENT_CONTRACT_READY` are required evidence inputs where named in
the workstream map or phase plan. The detailed `SV-G*` strings in the expanded
roadmap are internal evidence-bundle references only; they never supersede
these exact master tokens.

`SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` is the next gate after the current
documentation blueprint. It records reviewed evidence for the shared platform
runtime/service lifecycle, filesystem capability boundary, two deployment modes,
pairing/remote-access layers, progressive disclosure, and first-class host
matrix. It does not claim that a native installer, managed PostgreSQL package,
desktop client, mobile client, or hosted relay exists.

For unambiguous handoff, the expanded checkpoints map as follows:

| Expanded evidence reference | Master-plan promotion meaning |
|---|---|
| `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` | Records the Phase 0A contract/evidence boundary and is required before `SYNVEIL_FOUNDATION_READY`. |
| `SV-G0-FOUNDATION` | Contributes to `SYNVEIL_FOUNDATION_READY`; `SYNVEIL_BLUEPRINT_COMPLETE`, `SYNVEIL_CONTRACTS_READY`, and the cross-platform gate remain separate prerequisites. |
| `SV-G1-STORAGE` | Contributes to `SYNVEIL_FILE_STORAGE_READY` after `SYNVEIL_STORAGE_FOUNDATION_READY`. |
| `SV-G2-DATA-SAFETY` | Contributes to `SYNVEIL_UPLOADS_RELIABLE` and `SYNVEIL_DATA_INTEGRITY_READY`. |
| `SV-G3-TRUSTED-ACCESS` | Contributes to `SYNVEIL_DATA_LIFECYCLE_READY` once versions, Trash, sharing, and device authorization are integrated. |
| `SV-G4-SYNC-CONFORMANT` | Contributes to `SYNVEIL_SYNC_PROTOCOL_STABLE`. |
| `SV-G5-RESTORABLE` | Contributes to `SYNVEIL_BACKUP_RESTORE_READY`. |
| `SV-G6-OPTIMIZED-SAFELY` | Contributes to `SYNVEIL_STORAGE_OPTIMIZED`. |
| `SV-G7-DESKTOP-REFERENCE` | Contributes to `SYNVEIL_DESKTOP_SYNC_READY`. |
| `SV-G8-PHOTOS-SAFE` | Contributes to `SYNVEIL_PHOTOS_FOUNDATION_READY`. |
| `SV-G9-APPLE-CONTRACT-PROTOTYPE` | Prototype evidence only; it does not promote `SYNVEIL_APPLE_CLIENT_READY`. |
| `SV-G10-AI-OPTIONAL` and final AI evidence | Contribute to `SYNVEIL_AI_SEARCH_READY`; `SYNVEIL_AI_FOUNDATION_READY` is the workstream gate. |
| `SV-G11-FORGEJO-RESTORABLE` and `SV-G12-CODE-INTELLIGENCE` | Together contribute to `SYNVEIL_CODE_INTEGRATION_READY`. |
| Phase 12B Apple evidence | Promotes `SYNVEIL_APPLE_CLIENT_READY` only after the listed master prerequisites. |

This mapping lets a coordinator keep the detailed evidence vocabulary without
mistaking a local checkpoint for a master-plan authorization.

## Branch, commit, pull-request, and database rules

### Branch strategy

`main` stays buildable and is the only long-lived integration branch. Feature
work uses short-lived branches such as `feat/storage-object-store`,
`feat/resumable-upload`, `feat/sync-change-journal`, or
`feat/web-file-browser` (with any repository-required namespace prefix added
by the hosting workflow). Do not maintain permanent team branches. Use an
isolated worktree when two agents need unrelated paths, and name the one
integration owner for shared contract files.

### Commit policy

Commits are scoped and reviewable, for example:

```text
storage: add atomic object writer
sync: add monotonic change sequence
web: implement upload queue
docs: define backup retention semantics
```

Do not combine unrelated AI, database, UI, and sync changes in one commit.
Pair English/Vietnamese contract edits in the same documentation change set;
record generated artifacts and the source revision that produced them. Never
rewrite a released migration or stored format to make history look cleaner.

### Pull-request requirements

Every meaningful PR states:

```text
Objective
Scope
Architecture impact
API impact
Database impact
Security impact
Tests
Migration impact
Rollback considerations
```

Storage, sync, and backup PRs additionally state data-loss risk, crash
behavior, and retry behavior. The PR identifies affected ADRs/specs, gate
evidence, status changes, documentation parity, and known limitations. A
reviewer reproduces critical evidence or records why it could not be run.

### Database ownership rule

There is one authoritative migration sequence and one named migration
integrator. No team creates competing migration numbers or independently
changes the same relationship. The required order is:

```text
domain proposal → architecture review → migration → domain/OpenAPI update
→ implementation → compatibility/recovery tests
```

Released migrations are immutable and forward-only. Backfills are bounded,
observable, resumable, and rollback-aware; no manual production SQL is a
substitute for a reviewed migration. Database ownership does not grant a team
permission to redefine domain semantics owned by Architecture or the relevant
protocol team.

### Protocol-change rule

Any breaking change to sync, upload, authentication, object addressing, or
backup format requires an ADR, compatibility analysis, migration strategy,
fixtures, and tests. Clients may be offline for weeks or months, so the server
must define mixed-version behavior, capability negotiation, stale-cursor
recovery, and reader-before-writer rollout. A task may not silently change a
wire enum, cursor meaning, error code, credential scope, or object format.

## Definition of Done and conflict priority

“Works on my machine” is not a completion criterion. A production-oriented
feature is done only when implementation, unit tests, integration tests,
relevant failure/recovery cases, API docs, English docs, Vietnamese docs,
security review, logs/metrics, migration verification, backward-compatibility
analysis, and critical-TODO review all pass. A required skipped test or
unresolved critical risk leaves the work `PARTIAL` or `BLOCKED`.

When feature scope conflicts with safety, choose in this order:

```text
1. prevent data loss
2. prevent security violation
3. preserve data integrity
4. preserve protocol compatibility
5. preserve correct sync/backup behavior
6. reliability
7. performance
8. UX
9. advanced features
```

AI and polished UI never outrank integrity, authorization, or recoverability.

## Team communication contract

Every completion report uses this structure; “Done” alone is invalid:

```text
Verdict
Workstream
Prerequisite gate
Files changed
Architecture decisions
API changes
Database changes
Security impact
Tests executed
Failures / limitations
Open decisions
Next recommended gate
```

The expanded report may add performance, privacy, durability, compatibility,
environment, migration, and rollback evidence, but it must retain these
fields. Reports distinguish `COMPLETE`, `PARTIAL`, and `BLOCKED`; they never
claim a phase gate before the integration owner and reviewer record evidence.

## Recommended allocation and first implementation sequence

The master plan scales down without removing ownership boundaries:

| Team size | Allocation |
|---|---|
| 2 developers | Developer 1: Architecture + Platform + Rust + Storage + Sync. Developer 2: Web/Accessibility + DevOps/Installer + QA + Security. AI and integrations later. |
| 3–4 developers | Platform/Architecture; Backend/API; Storage + Sync + Backup; Web/UX; DevOps/Installer + Security + QA. |
| 5–7 developers | Platform/Architecture/API; Storage; Sync/Backup; Web/Accessibility; DevOps/Installer/Connectivity; Security/QA; AI/Integrations. |
| Multi-agent | Platform architect, Backend, Storage Platform, Sync, Web/Accessibility, Installer/Connectivity, Security, and QA agents with isolated scope; platform contract goes first. |

After `SYNVEIL_BLUEPRINT_COMPLETE` and the applicable contract/foundation
gates, issue one bounded implementation task at a time in this dependency
sequence:

1. cross-platform product/runtime/storage-capability contract and fixtures;
2. repository foundation and Rust workspace;
3. distribution/service/update/uninstall/migration/recovery evidence;
4. PostgreSQL and migration foundation;
5. authentication foundation;
6. ObjectStore abstraction and local adapter;
7. file/directory metadata model;
8. streaming upload/download;
9. resumable upload protocol;
10. integrity and atomic completion;
11. versioning and Trash;
12. sharing;
13. device model and pairing;
14. change journal;
15. sync protocol;
16. conflict resolution;
17. backup model;
18. snapshots and restore;
19. compression;
20. whole-object deduplication;
21. accessible web core and progressive-disclosure recovery UX;
22. desktop, mobile, Photos, AI, and Code integrations in their gated order.

The sequence may be split into smaller tasks, but a later task cannot use an
unstable contract as if it were frozen. Prototype work for future phases must
use synthetic data or an explicitly disposable format and cannot be promoted
by implication.

## Rule that must never be broken

No subsystem may perform an irreversible mutation merely because it decided
locally that the mutation seems appropriate. For garbage collection, dedup
cleanup, retention deletion, storage migration, repair, and similar operations
use:

```text
inspect → plan → validate → execute → verify
```

Provide a dry-run before destructive execution wherever practical. The plan
must state the exact references, leases, retention/legal holds, authorization,
rollback limits, audit record, and verification evidence. If inspection or
validation is uncertain, stop and preserve the data.

## Final team objective

The objective is not to maximize feature count or implementation speed. It is
to build Synveil into a self-hosted system that users can safely trust with
irreplaceable personal data. A smaller Synveil with reliable storage, sync,
backup, and restore is more valuable than a feature-rich system that can
silently lose or corrupt data.
