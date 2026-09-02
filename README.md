# Synveil

> **Your data. Your devices. Your cloud.**

Synveil is a planned private cloud anyone can self-host for owning, protecting,
synchronizing, organizing, and understanding files, backups, photos, devices,
and development projects. It is designed to feel approachable to non-technical
users while retaining advanced self-hosting control, and to remain useful
without a mandatory hosted Synveil service or AI.

> [!IMPORTANT]
> Synveil is currently in the **foundation stage**. This repository contains a
> validated Rust/API foundation, React/Vite web skeleton, optional isolated AI
> package boundary, and cross-platform CI. Browser-session HTTP authentication,
> the first-run bootstrap HTTP boundary, the minimal web setup/login/session
> shell, and the authenticated logical file/folder metadata slice are
> implemented and validated. Persisted resumable upload sessions and their
> transport-neutral application service, authenticated exact-offset HTTP
> upload transport, typed browser upload helpers, and a transport-neutral,
> owner-authorized immutable content-read application service over verified
> `ObjectStore` replicas are implemented and validated. The content service
> supports full reads and application-level validated ranges. Authenticated
> current-node and historical-version HTTP downloads now provide full and
> single-range streaming, strong SHA-256 validators, safe attachment headers,
> and private no-store caching. Authenticated immutable version-history listing
> and direct metadata lookup are also implemented and validated. Safe
> historical-version restore is implemented at the authenticated
> API/metadata boundary as one new immutable FileVersion reusing the verified
> historical Object; its disposable-PostgreSQL end-to-end gate remains
> environment dependent. Download UI, sync, backup, sharing, and deployment
> capabilities remain out of scope for this phase. Every other
> product capability below remains a blueprint unless explicitly marked
> otherwise.

## Status vocabulary

| Label | Meaning |
|---|---|
| `IMPLEMENTED` | Shipped in this repository and covered by validation. |
| `SKELETON IMPLEMENTED` | A foundation boundary exists, but it is not a product capability. |
| `VALIDATED` | The applicable foundation checks have passed; this does not promote a product feature. |
| `IN PROGRESS` | Actively being built but not a stable product capability. |
| `PLANNED` | Specified in the blueprint; implementation has not been completed. |
| `EXPERIMENTAL` | A future exploration with an unstable contract. |
| `NON-GOAL` | Deliberately outside the stated scope. |

Current repository status: **foundation skeleton, browser-auth transport,
first-run bootstrap HTTP flow, minimal web auth UI, logical file/folder
metadata API, authenticated resumable upload HTTP transport, authenticated
immutable version-history metadata, metadata-only Trash retention/purge
execution and reference accounting, a transport-neutral owner-authorized
immutable content-read service, and authenticated HTTP full/single-range
download transport implemented and validated**. The metadata slice covers
owner-scoped library/root listing, directory creation, node reads, rename,
move, logical delete, and restore. The upload slice streams bounded raw chunks,
resumes at a server-authoritative exact offset, and commits one verified
version through the upload service. Version history lists immutable records
newest-first with owner-scoped cursors and direct IDs compatible with
historical downloads. Safe historical-version restore appends one new
current FileVersion under CSRF, If-Match, owner/file, verified-replica, and
persisted idempotency controls; it never mutates historical rows, copies
bytes, moves the current pointer backward, or creates an UploadSession. The
Trash contract stores one server-observed `trashed_at`, derives a configurable
30-day-by-default restore deadline, exposes bounded internal eligibility, and
only begins the metadata `PURGING` state after owner/revision/transaction
checks. It does not delete physical bytes or implement physical purge/GC.
Metadata purge execution is an internal trusted operation: after a node is
`PURGING`, it removes that node and its `FileVersion` rows transactionally,
records metadata-only unreferenced-object candidates. The internal physical-GC
execution service is now implemented: a `READY` candidate with a live
lease/generation is revalidated transactionally, a durable operation and one
action per replica are recorded, and exact replicas are reconciled and deleted
one at a time through `ObjectStore`. The object enters `GC_DELETING`, which
rejects new durable references; metadata is removed only after every replica is
confirmed absent. The opt-in internal `synveil-worker` now drives bounded
planning/recovery cycles through those accepted services: it resumes durable
work before starting new work, persists retry scheduling, and records
metadata-only reconciliation findings. It has no HTTP route or normal-user
control surface, and it does not implement unknown-physical-orphan deletion,
sync, backup, or sharing. The
content service resolves active
owner-authorized current content or an allowed immutable historical version to
a verified replica, streams full bytes or a validated application-level range,
and cross-checks object metadata before yielding bytes. The HTTP download
routes keep filename/content-type/ETag/cache policy at the API boundary and
stream through the content service. They do not provide upload UI, download UI,
physical-GC controls, journals, sync, backup, or sharing.

The initial PostgreSQL canonical schema and explicit SQLx persistence mappings
for the validated domain subset are implemented. Disposable PostgreSQL
integration evidence remains environment-dependent and is reported separately;
Argon2id password hashing, persistent first-admin bootstrap, verifier-only
browser-session persistence, transport-neutral login/session semantics, HTTP
login/logout/session/CSRF and first-run bootstrap routes, secure cookie policy,
the minimal typed web API boundary, and the web setup/login/session shell are
implemented; PostgreSQL end-to-end evidence remains environment-dependent and
is reported separately. Device credentials, recovery, download UI, journals,
sync, backup, and sharing remain planned.
Deployment-managed first-run secret delivery and
installer lifecycle remain future work; the current bootstrap HTTP contract
does not accept a setup-secret field.

## Product direction

Synveil's product north star is:

> **A private cloud anyone can self-host.**

The first-class audience includes technical users, power users, families, home
users, students, creators, developers, small teams, and non-technical users.
The intended experience is `install → choose storage → create account → connect
devices → ready`. The architecture uses **Personal / Home Mode** for guided,
installer-based operation and **Advanced / Server Mode** for Docker Compose,
external PostgreSQL, NAS, S3/MinIO, custom networking, CLI, and detailed
diagnostics. Both modes use the same protocol and data-correctness model.

The product principles are **Easy by default**, **Safe by default**,
**Cross-platform by design**, **Powerful when needed**, and the existing
**correctness before cleverness**. A core user-facing feature should not require
an ordinary user to open a terminal. These are planned product goals, not
implemented capabilities.

Synveil combines several related, but explicitly separated, domains:

- `IMPLEMENTED/VALIDATED` (metadata, exact-offset upload, authenticated
  immutable version-history metadata, safe historical-version restore, Trash
  retention metadata/purge eligibility, authenticated full/single-range
  content-download transport, metadata-only GC planning, and physical
  Object/ObjectReplica GC) plus `IMPLEMENTED` bounded internal GC-worker
  orchestration/reconciliation / `PLANNED` (download UI and broader lifecycle)
  —
  authenticated files and folders, logical metadata, bounded streaming
  resumable upload, immutable full/range content reads, versions, trash,
  sharing, and integrity checks;
- `PLANNED` — first-class devices and a cursor-based multi-device sync
  protocol that preserves conflicting user data;
- `PLANNED` — backup sets, committed snapshots, retention, and verified
  restore, with deletion behavior distinct from sync;
- `PLANNED` — photo originals, renditions, timeline, albums, and mobile upload;
- `PLANNED` — whole-object deduplication and policy-based Zstandard
  compression;
- `PLANNED` — Forgejo repository inventory and backup through an integration
  boundary rather than a new Git server;
- `PLANNED` — optional asynchronous OCR, tagging, and semantic search using
  local or explicitly configured remote models;
- `PLANNED` — guided/native installation direction for Windows, macOS, Linux
  Desktop, and Linux Server, with storage selection, service lifecycle,
  human-readable health, safe updates, migration, and data-preserving
  uninstall;
- `PLANNED` — simple device pairing and understandable remote-access paths,
  with no mandatory proprietary Synveil relay;
- `PLANNED` (advanced) — files on demand, content-defined chunking, and
  deterministic smart tiering behind later phase gates;
- `EXPERIMENTAL` — repository/model intelligence experiments, anomaly
  detection, and multi-node scale-out.

The canonical feature catalogue and status are in
[docs/en/FEATURES.md](docs/en/FEATURES.md). The Vietnamese edition is in
[docs/vi/FEATURES.md](docs/vi/FEATURES.md).

## Architectural shape

The first production shape is a modular monolith, not a premature collection
of microservices:

```mermaid
flowchart LR
    Clients["Web and future device clients"] --> Runtime["Platform runtime / Advanced edge"]
    Runtime --> API["Rust API\nAxum + Tokio + Tower"]
    API --> DB[(PostgreSQL metadata)]
    API --> Store[(ObjectStore\ncapability-aware)]
    API --> Outbox[(Transactional outbox/jobs)]
    Worker["Rust background worker"] --> Outbox
    Worker --> Store
    AI["Optional Python AI worker"] --> Outbox
    AI --> DB
```

Core mutations commit metadata, a per-library change journal entry, audit
information, and durable work in one PostgreSQL transaction. File bytes are
made durable in the object store before metadata can reference them. Optional
consumers run after the core operation succeeds. If AI or Forgejo is offline,
file, sync, backup, and restore operations remain available.

The preferred implementation stack is:

- React, TypeScript, and Vite for the authenticated web application;
- Rust, Axum, Tokio, Tower, Serde, SQLx, and `tracing` for the server and core
  workers;
- PostgreSQL for metadata, journals, jobs, audit state, and—later—`pgvector`;
- an `ObjectStore` abstraction with local filesystem support first and
  S3-compatible/MinIO/NAS-backed adapters later;
- Python for optional, asynchronous ML/OCR/embedding workers;
- Docker Compose and Caddy for Advanced / Server Mode;
- future native installers and OS service adapters for Personal / Home Mode;
- PostgreSQL remains canonical; Personal / Home Mode is intended to manage its
  database dependency without asking ordinary users to administer it.

See [Architecture](docs/en/ARCHITECTURE.md),
[Platform](docs/en/PLATFORM.md),
[Storage](docs/en/STORAGE.md), [Sync](docs/en/SYNC.md), and
[Backup](docs/en/BACKUP.md) for the invariants and failure behavior.

## Blueprint map

| Area | English | Tiếng Việt |
|---|---|---|
| Product and scope | [PRODUCT](docs/en/PRODUCT.md) | [PRODUCT](docs/vi/PRODUCT.md) |
| Platform and accessibility | [PLATFORM](docs/en/PLATFORM.md) | [PLATFORM](docs/vi/PLATFORM.md) |
| System architecture | [ARCHITECTURE](docs/en/ARCHITECTURE.md) | [ARCHITECTURE](docs/vi/ARCHITECTURE.md) |
| Domain and API | [DOMAIN_MODEL](docs/en/DOMAIN_MODEL.md), [API_ARCHITECTURE](docs/en/API_ARCHITECTURE.md), [OpenAPI contract](api/openapi.yaml) | [DOMAIN_MODEL](docs/vi/DOMAIN_MODEL.md), [API_ARCHITECTURE](docs/vi/API_ARCHITECTURE.md), [OpenAPI contract](api/openapi.yaml) |
| Data safety | [STORAGE](docs/en/STORAGE.md), [UPLOADS](docs/en/UPLOADS.md), [SYNC](docs/en/SYNC.md), [BACKUP](docs/en/BACKUP.md) | [STORAGE](docs/vi/STORAGE.md), [UPLOADS](docs/vi/UPLOADS.md), [SYNC](docs/vi/SYNC.md), [BACKUP](docs/vi/BACKUP.md) |
| Product extensions | [PHOTOS](docs/en/PHOTOS.md), [AI](docs/en/AI.md), [CODE_INTEGRATION](docs/en/CODE_INTEGRATION.md) | [PHOTOS](docs/vi/PHOTOS.md), [AI](docs/vi/AI.md), [CODE_INTEGRATION](docs/vi/CODE_INTEGRATION.md) |
| Delivery | [ROADMAP](docs/en/ROADMAP.md), [TEAM_PLAN](docs/en/TEAM_PLAN.md), [TESTING](docs/en/TESTING.md) | [ROADMAP](docs/vi/ROADMAP.md), [TEAM_PLAN](docs/vi/TEAM_PLAN.md), [TESTING](docs/vi/TESTING.md) |
| Operations and trust | [SECURITY](docs/en/SECURITY.md), [DEPLOYMENT](docs/en/DEPLOYMENT.md) | [SECURITY](docs/vi/SECURITY.md), [DEPLOYMENT](docs/vi/DEPLOYMENT.md) |
| Decision process | [ADRs](docs/adr/README.md), [CONTRIBUTING_ARCHITECTURE](docs/en/CONTRIBUTING_ARCHITECTURE.md) | [ADRs](docs/adr/README.md), [CONTRIBUTING_ARCHITECTURE](docs/vi/CONTRIBUTING_ARCHITECTURE.md) |

Most documents describe intended product contracts, not evidence of product
implementation. The status markers above identify the limited foundation
evidence that has been implemented and validated. The [repository audit](docs/en/REPOSITORY_AUDIT.md)
records the exact starting state.

## Proposed repository shape

Implementation phases are expected to grow the repository deliberately:

```text
apps/web                 React application
crates/                  Rust domain and adapter crates
services/ai              optional Python AI worker
clients/                 future device clients and shared Rust sync core
api/openapi.yaml         reviewed public contract
migrations/              forward database migrations
deploy/                  Compose, Caddy, and operational assets
docs/                    bilingual specifications and ADRs
scripts/                 repeatable developer/operations commands
tests/                   cross-component, recovery, and protocol suites
```

Directories should be introduced by scoped implementation tasks; this
blueprint does not create empty product scaffolding.

## Roadmap and contribution state

The first implementation gate is Phase 0: repository/workspace scaffolding,
CI, configuration boundaries, migration tooling, an OpenAPI skeleton, and a
cross-platform/service abstraction foundation. Compose remains the local
development topology and an Advanced / Server deployment option; it is not the
only eventual user installation path. Storage and sync implementation must not
begin by inventing contracts that conflict with accepted ADRs or platform
boundaries.

Start with [the roadmap](docs/en/ROADMAP.md), including the cross-platform
foundation sub-phase, then use
[the team plan](docs/en/TEAM_PLAN.md) to issue one gated task to an
implementation agent. Architectural changes follow
[the contribution protocol](docs/en/CONTRIBUTING_ARCHITECTURE.md).

External contributions are welcome after the Phase 0 contribution workflow is
implemented. Until then, design proposals should identify the affected ADR,
protocol specification, API surface, migration, tests, and bilingual
documentation.

## Licensing

The repository is currently licensed under the [MIT License](LICENSE). The
blueprint proposes evaluating AGPL-3.0 for the future core/server/web and
Apache-2.0 for selected SDKs, but **no relicensing has occurred**. That choice
requires an explicit owner decision and a contributor-rights review; see
[ADR-011](docs/adr/ADR-011-open-source-licensing.md).
