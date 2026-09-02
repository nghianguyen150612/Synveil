# Synveil feature catalogue

Status: **PLANNED product blueprint**

This catalogue defines intended product scope; it is not a claim that any
capability is available. The repository contains foundation scaffolding but no
product feature implementation. A feature remains `PLANNED` or `EXPERIMENTAL` until the
evidence gate in
[Contributing to Synveil architecture](CONTRIBUTING_ARCHITECTURE.md) permits a
reviewed status change.

## Reading the catalogue

The status words are normative:

| Label | Meaning here |
|---|---|
| `PLANNED` | Intended and contractually bounded, but not implemented. |
| `EXPERIMENTAL` | Research or an unstable opt-in surface with no compatibility promise. |
| `NON-GOAL` | Deliberately outside the stated horizon. |

`Core`, `Near-term`, `Advanced`, and `Experimental/Future` describe dependency
and product priority, not current availability. Roadmap phases, prerequisites,
and exit gates live in [ROADMAP.md](ROADMAP.md); implementation owners consume
[TEAM_PLAN.md](TEAM_PLAN.md).

The following rules apply to every feature:

1. Core file, sync, backup, version, and restore correctness does not depend on
   AI, thumbnailing, OCR, search enrichment, repository polling, or another
   optional worker.
2. PostgreSQL is authoritative for metadata and transactional state. Canonical
   file bytes are immutable `Object` values held by an object-store adapter.
3. User-visible identity follows `Library → Node → FileVersion → Object` as
   specified in [DOMAIN_MODEL.md](DOMAIN_MODEL.md).
4. Every client-visible mutation is authorized from the authenticated
   principal and resource relationship, conditionally applied, safe to retry
   under the documented idempotency contract, and journaled when it changes a
   `Library`.
5. A backup snapshot is a retention-controlled historical manifest. Missing
   backup input never means “delete the live cloud node.”
6. Optional derived data is replaceable. Losing it may degrade a feature but
   must not lose, mutate, or make canonical bytes inaccessible.

## Core capabilities

Core capabilities establish a safe self-hosted data platform. Every item below
is `PLANNED`.

### Identity, authentication, and accounts

- Bootstrap exactly one initial administrator through a one-time,
  deployment-local setup flow; close or explicitly re-arm bootstrap after
  success.
- Create and administer user accounts without inventing custom cryptography.
  Passwords use a reviewed memory-hard password hashing scheme with versioned
  parameters.
- Log in and out; issue, rotate, refresh, revoke, and expire sessions without
  storing plaintext bearer credentials.
- Register device-specific credentials, show last-seen/security activity, and
  remotely revoke a device's Synveil credentials.
- Provide authenticated API access, per-principal rate limits, account recovery
  with an explicit self-hosted operator policy, and security audit events.
- Add MFA only after recovery, enrollment, revocation, and anti-lockout
  behavior are designed and threat-reviewed.

The system does not claim to wipe an operating system. A device revocation may
invalidate credentials and request deletion of a Synveil-managed cache only.
The security and trust-boundary contract belongs to
[SECURITY.md](SECURITY.md).

### Files, folders, and metadata

- Upload and stream-download files, including HTTP byte ranges, without
  buffering a complete large object in API or worker memory.
- Create folders; list children with stable keyset pagination; and filter,
  sort, search, select, or operate on multiple nodes within bounded limits.
- Rename, move, and copy logical nodes. Rename and move update metadata rather
  than relocating immutable multi-gigabyte content.
- Preserve MIME type as untrusted metadata, byte size, creation and
  modification timestamps, client-supplied filename, integrity state, and
  version identity.
- Soft-delete to trash, restore, and eventually purge under explicit retention
  and storage-accounting rules. Recursive operations are asynchronous or
  otherwise bounded rather than one unbounded transaction.
- Support browser drag-and-drop as a client behavior over the same upload
  protocol, not as a separate storage path.

Names are untrusted display data. They never become object-store keys or
server filesystem paths without adapter-owned encoding and validation. Exact
storage behavior is specified in [STORAGE.md](STORAGE.md).

### Logical objects, integrity, and reliable upload

- Keep `Node`, immutable `FileVersion`, and immutable `Object` separate.
- Support local filesystem, NAS-mounted filesystem, MinIO, and other
  S3-compatible backends behind one object-store contract; add future adapters
  only with conformance and recovery tests.
- Initiate a resumable `UploadSession`, accept independently retriable parts,
  verify declared lengths and checksums, assemble or finalize durably, and
  atomically expose a new version.
- Expire and reclaim abandoned staging data without touching committed
  objects.
- Use SHA-256 as the initial canonical plaintext integrity and whole-object
  deduplication hash. Storage keys remain opaque; hashes and user paths are not
  exposed through them.
- Handle disk-full, quota exhaustion, checksum mismatch, process crash,
  duplicate completion, lost success responses, and an object write followed
  by a failed database commit.

Only a durable, verified object may be referenced by a successful content
commit. That database transaction also appends the library change, audit fact,
and durable outbox work. The protocol details live in
[UPLOADS.md](UPLOADS.md).

### Versions, trash, and recovery

- Create a new immutable `FileVersion` for content replacement; never rewrite
  an old version in place.
- Browse prior versions, restore a prior version by creating a new head, and
  retain source attribution and timestamps.
- Share physical bytes between versions only inside the same deduplication
  domain and without weakening authorization.
- Apply independent version and trash retention rules. Purging a logical
  reference does not delete shared physical bytes until no live, historical,
  backup, derivative, or staging reference remains and a safety window passes.
- Quarantine corrupt objects, surface affected versions, and support verified
  restore rather than silently serving known-bad bytes.

### Multi-device synchronization

- Give every `Library` one transactionally incremented ordered change sequence
  and an opaque, versioned `SyncCursor`.
- Synchronize create, content modify, rename, move, trash, restore, and purge
  facts through
  `GET /api/v1/libraries/{library_id}/changes?cursor=...`.
- Require base version or ETag preconditions for writes. A lost response can be
  replayed with its idempotency key and returns the original outcome.
- Preserve both results of concurrent offline content edits. The first valid
  write advances the head; the stale content write becomes the deterministic
  conflict copy required by ADR-006, never a silent overwrite.
- Detect an expired, malformed, wrong-library, or wrong-epoch cursor. A client
  then performs a paginated authoritative rescan and resumes from a server
  checkpoint without inferring ordering from UUIDs or timestamps.
- Define deterministic behavior for concurrent directory moves, cycle
  prevention, name collisions, deletes versus edits, restore versus purge, and
  retries after partial client application.

Planned policies are two-way sync, upload-only backup, download-only mirror,
cloud-only, pinned locally, and excluded. “Upload-only backup” invokes backup
semantics; it is not a deletion-propagating sync mode. Participation and local
materialization rules may be scoped by device, `Library`/directory subtree,
file type, and explicit user choice, while the protocol still delivers the
metadata and tombstones required for correct convergence across filter moves.

### Backup, snapshots, and restore

- Define device-scoped `BackupSet` values for selected sources, schedule or
  continuous capture, exclusions, and retention.
- Build immutable `BackupSnapshot` manifests and expose only snapshots that
  reached `COMMITTED` after all referenced objects and manifest integrity were
  verified.
- Retain historical entries when input disappears, according to policy.
  Absence is recorded as a backup observation, not emitted as a live `Node`
  deletion.
- Restore one file, a directory subtree, a prior file version, or a complete
  snapshot to an explicit destination; do not overwrite newer data without a
  reviewed precondition and user choice.
- Recover after loss or revocation of a source device, and verify restored byte
  count, canonical hash, manifest membership, and skipped/conflicted entries.
- Bound interrupted scans, repeated submissions, unchanged-file reuse,
  changed-large-file handling, quota failure, corrupt-object handling, and
  retention cleanup.

[BACKUP.md](BACKUP.md) owns the snapshot and restore protocol. Backup data may
reuse `Object` values within a permitted deduplication domain, but sync state
and snapshot state remain separate.

### Device management

- Register a device with an authenticated user, stable opaque ID, display
  label, platform/capability declaration, and independently revocable
  credential.
- Display last successful contact, credential state, assigned library policy,
  sync checkpoint, backup status, and storage/cache status without treating
  client claims as trusted security facts.
- Pause sync or backup by policy, resume safely, show lag and errors, and keep
  an auditable activity history.
- Revoke every server credential for one device immediately and optionally
  queue a best-effort request to delete only Synveil-managed local cache. The
  request may never reach an offline/compromised client and is not an operating
  system remote-wipe claim.
- Negotiate protocol and capability versions so clients lacking native
  placeholders, background execution, or another feature receive a compatible
  policy.

### Deterministic search and operations

- Search filenames and typed metadata without AI; add full-text search for
  extracted or explicitly indexed text when its parser and access controls pass
  their gates.
- Keep semantic, photo, and code search as separate optional layers and label
  result provenance.
- Expose structured logs, request IDs, traces, metrics, liveness, readiness,
  database health, storage health, and worker lag while excluding secrets and
  file contents.
- Support Docker Compose as a production topology, with Caddy as an optional
  edge proxy rather than an internal dependency.
- Back up and restore Synveil metadata and object storage with a documented,
  tested compatibility point before an upgrade.

### Installation, onboarding, health, and recovery

The core product must be approachable without making the correctness boundary
smaller:

- Provide a guided Personal / Home installation for Windows, macOS, Linux
  Desktop, and Linux Server profiles when each platform passes its release gate.
- Let a non-technical user choose a storage location, see capacity and health,
  create the first account, pair a device, and understand backup/recovery
  obligations without manually configuring PostgreSQL, SQL, Compose, a reverse
  proxy, or environment-variable secrets.
- Keep an Advanced / Server path with Compose, external PostgreSQL, custom proxy/
  TLS, NAS/S3, CLI, and operator diagnostics using the same API, data model, and
  storage correctness rules.
- Translate stable errors into a next action for ordinary users while retaining
  administrator and developer diagnostics behind progressive disclosure.
- Provide data-preserving update, uninstall, reinstall, machine migration, and
  recovery workflows. Application removal and permanent data deletion are never
  the same action.
- Use one short-lived authenticated pairing flow for browser, desktop, and
  future mobile clients. Remote access is optional and self-hosted-first; a
  hosted relay is not a core dependency.
- Treat automatic maintenance, service recovery, signed updates, storage
  capability discovery, and health checks as product behavior rather than
  undocumented operator folklore.

## Near-term product capabilities

These items are `PLANNED` after the storage foundation.

### Sharing

- Share privately with another user as read-only or writable, subject to the
  resource owner's policy and the recipient's effective permission.
- Create revocable public links with scope, expiration, optional password, and
  abuse limits. Store password verifiers, never plaintext passwords.
- Audit create, access, permission change, failed password attempt, and
  revocation without logging share secrets.
- Reject insecure direct object reference attempts: possession of a `Node`,
  `Object`, or `Share` ID alone grants no access.
- Revoke access immediately at authorization time. Cached or already
  downloaded bytes cannot be cryptographically recalled and the UI must not
  imply otherwise.

### Whole-object storage optimization

- Apply policy-based Zstandard compression only to measured, eligible content
  such as text, JSON, CSV, logs, database dumps, and source code.
- Skip formats that are already compressed, encrypted, or unsupported unless
  a benchmark proves a bounded benefit.
- Record codec, codec version/parameters, encoded length, plaintext length,
  and plaintext hash so decompression is transparent and verifiable.
- Deduplicate equal canonical plaintext only within one ownership/deduplication
  domain. Do not reveal equality across users through timing, quota, errors, or
  object IDs.
- Account logical, retained, and physical bytes separately; never promise a
  user's “saved space” from another user's private data.

Compression precedes server-side application encryption when both are used.
Zero-knowledge encryption, cross-user deduplication, server-side preview, and
server-side AI are not simultaneously assumed.

### Photos

- Ingest original image and video resources through the normal durable upload
  path, then asynchronously extract safe metadata and generate derivatives.
- Provide a timeline, albums, favorites, videos, screenshots classification,
  search, exact-byte duplicate suggestions, and per-device backup state.
- Treat perceptual near-duplicate suggestions as `EXPERIMENTAL`, separately
  consented derived data that cannot delete, merge, or change retention.
- Preserve EXIF as original metadata while treating location, faces, and
  capture context as sensitive derived fields.
- Group Live-Photo-like resources without pretending that every client or
  format has the same representation.
- Prepare PhotoKit and background-transfer contracts for a future Apple client,
  including limited-library access, device-scoped source identifiers, retry,
  and iOS scheduling constraints.

No photo processor may rewrite the canonical original. Details and privacy
controls live in [PHOTOS.md](PHOTOS.md).

## Advanced capabilities

Advanced items are `PLANNED`, but they must not block early releases.

### Files on demand and smart sync

- Represent server-side logical state needed by clients to map `Local`,
  `Cloud-only`, `Pinned`, `Downloading`, `Uploading`, `Conflict`, and
  `Unavailable`.
- Let a capable desktop or Apple FileProvider client hydrate by immutable
  version and safely evict only a verified local cache.
- Make pinning and exclusion policy explicit per device. Never infer that an
  evicted local cache is a server deletion.
- Negotiate capabilities because Linux, Windows, macOS, iOS, and web do not
  expose identical placeholder APIs.

### Chunk-level deduplication

- Explore content-defined chunking, hash-indexed immutable chunks, and
  versioned manifests for VM images, datasets, game assets, and repeated large
  backups.
- Keep the canonical whole-object contract valid so clients and restores do
  not depend on a particular chunker.
- Bound chunk count, manifest size, collision verification, memory, random-read
  amplification, garbage collection, and format migration.
- Introduce a stored chunk format only through an accepted ADR and
  readers-before-writers compatibility plan.

### Desktop and Apple clients

- Reuse a reviewed Rust sync core across desktop clients if platform boundary
  and FFI experiments justify it; keep native placeholder and credential
  integration platform-specific.
- Plan Linux, Windows, and macOS selected-folder sync, selected-folder backup,
  tray/status UX, bandwidth control, and recovery.
- Plan Swift, SwiftUI, FileProvider, PhotoKit, URLSession, Keychain, and Swift
  Concurrency for Apple clients.
- Reserve future Android, iPhone, and iPad clients for the same protocol and
  device/pairing model, with OS-specific background, filesystem, notification,
  credential-store, and photo-library boundaries. Mobile is not required for
  the first cross-platform foundation gate.
- Authenticate, register a device, select policies, perform an initial
  snapshot, then consume ordered changes without redesigning the server.

### Code integrations and projects

- Connect to Forgejo first; list repositories and metadata, summarize
  branches/tags/recent commits, report health and storage use, and associate
  repositories with optional `Project` workspaces.
- Back up and verify repository Git data, Git LFS objects, and explicitly
  supported release artifacts; restore to a safe destination without
  overwriting a live repository by default.
- Keep Git smart HTTP/SSH, packfiles, refs, permissions, pull requests, and
  issues under Forgejo ownership.
- Let a `Project` link repositories, documents, assets, backups, devices, and
  related metadata without changing their original ownership or deletion
  lifecycle.

The integration boundary and unavailable-Forgejo behavior are defined in
[CODE_INTEGRATION.md](CODE_INTEGRATION.md).

### Optional AI foundation

- OCR suitable PDFs, screenshots, scans, and photos; store extracted text
  separately from canonical bytes.
- Create version-bound embeddings for authorized text/code/OCR-derived chunks,
  including OCR text from photos, repository/code, and project scopes, and
  provide semantic retrieval as an optional layer. Vision/image embeddings
  remain experimental.
- Suggest editable automatic tags from text/code/OCR and reviewed deterministic
  metadata with provenance `USER`, `SYSTEM`, or `AI`. Vision-generated
  scene/object/person tags remain `EXPERIMENTAL`.
- Support `DISABLED`, local/self-hosted, and explicitly consented
  remote-provider inference modes. Sensitive content never silently leaves the
  server.
- Expose source/model provenance, freshness, reindex, consent withdrawal, and
  deletion of derived data without changing canonical retention.

These Phase 10 capabilities are `PLANNED` advanced features. AI failure,
disablement, reindexing, or index deletion leaves upload, download, sync,
backup, restore, and deterministic search operational. The full contract is
[AI.md](AI.md).

### Smart storage tiering

- Begin with explainable `HOT`, `WARM`, `COLD`, and `ARCHIVE` policy rules based
  on explicit user choice, access time, size, device free space, network state,
  and file type.
- Treat placement as an asynchronous replica transition. Do not mark a source
  replica removable until the target is durable and verified.
- Preserve a reachable copy and restore path under backend outage, partial
  migration, or policy change.

## Experimental and future capabilities

These surfaces are `EXPERIMENTAL` unless promoted through an ADR and phase gate.

### AI and repository intelligence experiments

- Evaluate cited generated repository summaries, repository Q&A, and
  project-aware natural-language answers only over data the caller may
  currently access. Version-bound semantic code indexing/retrieval itself is
  the advanced `PLANNED` foundation above.
- Evaluate image captions, face/scene understanding, and model-driven
  recommendations as separately consented derived features.
- Add provider/model adapters only after license, privacy, deletion, resource,
  quality, and prompt-injection gates pass.
- Keep generated answers read-only and tool-free; any side-effecting agent
  requires a separate ADR and authorization design.

### Anomaly detection

- Detect unusual destructive patterns such as a burst of rewrites or deletes;
  preserve evidence, suggest pausing propagation, and ask for review.
- Use bounded heuristics before machine learning and expose the triggering
  observations.
- Never claim perfect ransomware detection or permanently block legitimate
  work without a recoverable operator path.

### Future scale-out and providers

- Add GitHub, GitLab, Gitea, remote AI providers, additional object stores, or
  distributed workers only behind existing contracts.
- Consider horizontal API replicas, a message broker, Kubernetes, and
  multi-node operations only after measured load or availability requirements
  justify their operational cost.

## Planned web information architecture

The authenticated React/TypeScript/Vite application is `PLANNED` to expose:

```text
Login
Dashboard
Files
  My Drive | Shared | Recent | Favorites | Trash
Photos
  Timeline | Albums | Search
Backups
  Devices | Snapshots | Restore | Policies
Devices
Code
  Repositories | Projects | Git Servers
Search
Activity
Settings
  Account | Storage | Security | Integrations | AI
```

Files → Favorites is backed by a per-user `NodeFavorite` relationship. It is
not shared node metadata, never grants access or retains content, and hides
nodes the caller can no longer read.

Files → Shared is populated from the caller's authorization-filtered received
share collection; users do not need to know a share or node ID in advance, and
revoked/expired grants disappear without leaking inaccessible ancestors.

Files → Recent is a snapshot-bounded list of currently readable nodes ordered
by committed server mutation time. It does not record previews/downloads as a
hidden per-user viewing history, and it immediately omits trashed or revoked
content.

Pages must display capability and degradation state truthfully. A navigation
shell or mock does not promote a feature to `IMPLEMENTED`.

## Cross-feature failure contract

| Failure | Required product behavior |
|---|---|
| Object store is unavailable | Reject new content commits with a stable retryable error; do not create visible versions that reference missing bytes. Metadata reads that need no bytes may continue if safe. |
| Object is durable but DB commit fails | Keep the object unreferenced and eligible for delayed orphan reconciliation; return failure. Never expose a `Node` version from an uncommitted transaction. |
| DB commit succeeds but response is lost | A retry with the same idempotency key returns the committed outcome and must not create another version or journal entry. |
| Disk fills during staging | Fail or pause the affected upload, preserve committed objects, report quota/disk health, and reclaim only validated staging data. |
| Optional worker is offline | Core writes succeed after durable outbox recording; derived state becomes `PENDING` or `STALE` and worker lag is visible. |
| PostgreSQL is unavailable | Reject mutations rather than writing untracked canonical objects as successful user data. |
| Stale sync cursor | Return a stable cursor error with a server-directed rescan path; never guess from client timestamps. |
| Backup source omits a former path | Apply snapshot/retention policy; do not emit a live deletion. |
| Forgejo is unavailable | Mark integration data stale, retry with bounds, and keep core storage and existing verified repository backups available. |
| AI is disabled or remote consent is withdrawn | Stop dispatch to that mode, revoke queued remote work where possible, and permit deletion/rebuild of derived records; canonical data remains usable. |

## Explicit early non-goals

The following are `NON-GOAL` for early versions:

- complete iPhone or operating-system backup;
- iMessage synchronization;
- a GitHub/Forgejo replacement or custom Git transport;
- an office suite or chat platform;
- a full media-transcoding platform;
- an enterprise IAM suite;
- a Kubernetes-native distributed filesystem;
- zero-knowledge encryption bolted onto server-side deduplication, previews,
  OCR, and AI without a separate architecture;
- mandatory remote AI or a proprietary hosted control plane.

## Feature promotion gate

A feature may move from `PLANNED` or `EXPERIMENTAL` only when:

1. its domain and protocol contract is accepted and OpenAPI is reviewed;
2. authorization, privacy, abuse, secret, and audit behavior is threat-reviewed;
3. correctness, retry, crash, recovery, and compatibility tests pass;
4. phase-relevant benchmarks and resource bounds are recorded without
   fabricated targets;
5. health, metrics, backup/restore, upgrade, and rollback limits are
   operationally documented;
6. English and Vietnamese documents match; and
7. no unresolved high-severity data-loss or authorization defect remains.
