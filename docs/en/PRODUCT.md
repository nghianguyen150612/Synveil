# Product definition

- Status: **Normative blueprint**
- Product: **Synveil**
- Tagline: **Your data. Your devices. Your cloud.**

## Definition

Synveil is a reliable private cloud anyone can self-host for owning, protecting,
synchronizing, organizing, and understanding personal or small-team data across
devices.

It brings files, version history, backups, devices, photos, selected developer
projects, and optional intelligence into one product while retaining explicit
subsystem boundaries. It is not a bundle of features that share an icon: the
common foundation is a verifiable object store, transactional metadata,
identity and authorization, an activity journal, and recovery-oriented
operations.

The product must work when no proprietary hosted Synveil service exists. Core
file, sync, backup, and restore operations must work when AI is disabled or
unavailable.

The intended product experience is consumer-cloud convenience with self-hosted
ownership, reliable synchronization, and safe backup/recovery. It must be
approachable to non-technical users without taking advanced control away from
self-hosters.

## Problem and promise

Users commonly split data across consumer clouds, device-specific backup
systems, NAS folders, photo libraries, removable disks, and code forges. This
creates unclear ownership, fragile restore paths, duplicate storage, and no
coherent view of which device or copy is current.

Synveil's promise is narrower and testable:

- users can locate their logical data and its history without understanding
  physical object keys;
- data accepted as committed is durable according to the configured storage
  and can be verified by checksum;
- synchronization never silently discards a concurrent content edit;
- backup retention does not inherit sync deletion semantics;
- self-hosted operators can observe health, move or restore their data using
  documented formats and procedures, and upgrade without wiping metadata;
- optional indexing makes owned data easier to find without becoming the only
  way to find it or a hidden data-export channel.

## Intended users and environments

First-class audiences are technical users, power users, families, home users,
students, creators, developers, small teams, and non-technical users. Synveil
may run on a Windows or macOS personal computer, Linux Desktop, Linux Server,
NAS, home server, VPS, or dedicated server. Personal / Home Mode is a future
guided/native installation experience; Advanced / Server Mode includes Docker
Compose and operator-controlled deployment. The initial Compose topology remains
one PostgreSQL primary and a configured local/NAS path, while S3-compatible
storage is a planned adapter rather than a requirement for public cloud.

Future users may connect:

- a browser for file, photo, backup, device, search, and administration views;
- desktop clients on Windows, macOS, and Linux for selected-folder sync and
  backup, ideally sharing a Rust protocol/state-machine core;
- future Android, iPhone, and iPad clients using platform-native APIs and
  realistic background/permission constraints;
- integrations such as Forgejo for repository inventory and backup.

Synveil is not initially an enterprise multi-tenant SaaS control plane or a
mandatory Synveil-hosted relay service.
Ownership and authorization still must be modeled rigorously so later shared
and hosted deployments do not require insecure assumptions.

## Product principles

### Easy by default

Normal users receive guided installation, storage selection, account bootstrap,
device pairing, health status, maintenance, and recovery language without being
asked to administer infrastructure. The primary product journey must not be
terminal-first.

### Safe by default

Automation may hide complexity, but it never hides destructive scope or weakens
durability, authorization, backup, update, or recovery guarantees.

### Cross-platform by design

Windows, macOS, Linux Desktop, and Linux Server are first-class host targets.
Platform service managers, credential stores, path semantics, and storage
accelerators remain behind adapters and capability contracts.

### Powerful when needed

Advanced users retain Docker Compose, custom PostgreSQL, S3/MinIO, NAS, custom
reverse proxies/TLS, CLI, API access, detailed diagnostics, and filesystem
optimization. See [PLATFORM.md](PLATFORM.md).

### Self-hosted first

One supported advanced production installation must remain possible with Docker
Compose, while the long-term user-facing deployment model also includes native
or guided Personal / Home installation. Kubernetes may become a scale-out guide
only after measured need; it cannot be a functional prerequisite.

### User ownership

Storage locations, retention, sharing, remote AI use, integrations, and
recovery behavior must be visible and configurable. Canonical file bytes may
not be trapped in PostgreSQL or an undocumented proprietary representation.

### Correctness before cleverness

Storage, upload, sync, backup, restore, trash, and version history take
precedence over recommendations, semantic indexing, advanced compression,
chunk deduplication, and tiering. An optimization that weakens durability or
recovery is rejected.

### Modular failure boundaries

AI, photo enrichment, repository polling, thumbnail generation, and other
consumers operate asynchronously. Their failure can make derived features
stale but cannot make committed original content unavailable.

### Honest status

Screens, documentation, release notes, and APIs use the shared `IMPLEMENTED`,
`IN PROGRESS`, `PLANNED`, `EXPERIMENTAL`, and `NON-GOAL` taxonomy. This
blueprint does not make a capability implemented.

### Portable contracts

Web, desktop, and mobile clients consume the same versioned HTTP contracts and
opaque identifiers. Server behavior cannot assume POSIX paths, unrestricted
iOS background execution, or that every platform implements native
placeholders.

### Progressive disclosure

Normal UI uses Files, Photos, Backups, Devices, Shared, Search, Activity,
Settings, and Health. PostgreSQL, object stores, sync cursors, GC, compression,
S3, reflink, and maintenance detail appear only in `Settings → Advanced` or
administrator diagnostics. A core user-facing feature should not require an
ordinary user to open a terminal.

## Product domains

| Domain | User outcome | Early criticality |
|---|---|---|
| Identity and devices | Know who and which device acts; revoke access. | Core |
| Drive | Organize and transfer files/folders with integrity. | Core |
| Versions and trash | Undo changes and recover soft-deleted content. | Core |
| Sync | Converge selected libraries without silent overwrite. | Core |
| Backup and restore | Preserve historical device data under explicit retention. | Core |
| Sharing | Grant bounded, revocable access and public links. | Near-term |
| Photos | Preserve originals and expose timeline/albums/renditions. | Near-term |
| Storage optimization | Reduce physical usage without changing logical truth. | Near-term/advanced |
| Search | Always provide metadata search; add full-text and semantic layers. | Near-term/advanced |
| Code integration | Associate Forgejo repositories and protected artifacts with projects. | Advanced |
| AI | Optional OCR, embeddings, tags, and repository understanding. | Advanced |
| Smart storage | Files on demand and deterministic tiering. | Advanced |
| Intelligence/scale experiments | Anomaly signals, model experiments, and multi-node scale-out. | Experimental |

`FEATURES.md` is the complete catalogue. Domain names are boundaries, not a
promise that each becomes an independent network service.

## User-visible information architecture

The web application can grow incrementally into:

```text
Login
Dashboard
Files / My Drive / Shared / Recent / Favorites / Trash
Photos / Timeline / Albums / Search
Backups / Devices / Snapshots / Restore / Policies
Devices
Code / Repositories / Projects / Git Servers
Search
Activity
Settings / Account / Storage / Security / Integrations / AI
```

Favorites are personal per-user bookmarks, not shared file metadata; they
never grant access or keep content alive after its retention references end.
The Shared view discovers currently authorized received grants without
requiring users to know internal share or node IDs.
Recent means currently readable nodes ordered by committed server mutation,
not an undisclosed history of what the user viewed or downloaded.

Early navigation must hide or clearly label unavailable sections. Empty mocks
must not imply that data protection is operational.

## Core user journeys

### Initial installation

```text
Personal / Home Mode:
  install
  → choose a storage location
  → Synveil-managed services and PostgreSQL
  → create the account
  → validate health
  → connect a device

Advanced / Server Mode:
  obtain a pinned release
  → configure database/object paths and secrets
  → start the documented Compose/native services
  → open the TLS-protected setup route
  → create the one-time bootstrap administrator
  → validate storage and database health
  → create the first Library
```

Ordinary setup must not require direct SQL, manual PostgreSQL administration,
environment-variable editing, reverse-proxy configuration, or port forwarding.
Advanced setup may expose those controls. All exact installers and packaging
commands remain `PLANNED` until release artifacts exist; `git clone` is a
development route, not the only production upgrade model.

### Future device onboarding

```text
install trusted client
→ scan/enter a short-lived pairing code after verifying server identity
→ approve and register a named Device
→ receive a scoped, revocable device credential
→ select distinct sync and/or backup policies
→ take a consistent initial inventory
→ start cursor-based changes and status reporting
```

Pairing remains security-sensitive: code lifetime, replay, approval, device
identity, credential issuance, revocation, local/remote pairing, and MITM
protection must be designed before convenience is promoted.

### Device-loss recovery

The user revokes the lost device credential, reviews audit activity, registers
a replacement device, chooses a committed `BackupSnapshot` or Drive history,
restores into a non-destructive destination, and receives checksum verification
results. Revoking Synveil credentials does not claim to wipe an operating
system.

### Remote access

Remote access is planned as a layered choice: LAN discovery, direct connections,
safe NAT traversal, optional user-owned VPN/tailnet integration, and manual
Advanced / Server networking. An optional relay/coordination service remains an
open, self-hosted-first decision. Synveil must remain useful without a
proprietary hosted dependency, and the UI must say when a connection is local,
ready, degraded, or needs network administration.

## Non-goals for early releases

Synveil will not initially be:

- a complete iPhone or full operating-system backup replacement;
- an iMessage synchronization service;
- a GitHub/Forgejo replacement or custom Git transport;
- an office suite, chat platform, or general workflow system;
- a Kubernetes-native distributed filesystem or consensus database;
- an enterprise IAM suite;
- zero-knowledge encrypted everything;
- a full media transcoding platform;
- a mandatory Synveil-hosted relay or control plane;
- an operating-system installer or custom filesystem in this blueprint phase.

These boundaries limit data-loss risk and keep core recovery testable.

## Success and release evidence

No invented throughput or scale number defines product success. A release gate
instead requires evidence that:

- the declared install topology can be deployed and upgraded from the previous
  supported version;
- accepted writes survive tested crash points and are either visible and
  verifiable or safely collectable as unreferenced staging data;
- retries after lost responses are idempotent;
- sync scenarios converge and preserve conflict bytes;
- backup snapshots obey retention independently from live deletions and restore
  verifies content;
- authorization and quota tests cover every mutation and download path;
- health, metrics, audit records, and recovery runbooks expose failures;
- a normal user can install, choose storage, pair a device, view health, update,
  uninstall without deleting data, migrate to another computer, and recover
  through guided flows once those capabilities are implemented;
- public status labels, English docs, and Vietnamese docs match the shipped
  evidence.

Performance has benchmark categories and regression budgets in `TESTING.md`;
targets are set from Phase 1 baselines on named hardware.

## Sustainable open-source direction

The intended core must remain useful for self-hosters without paid modules.
Possible commercial work includes managed hosting, support/SLA, managed backup,
and enterprise administration or SSO. Those services must not make users
dependent on a proprietary control plane for ordinary access or export.

The current legal license is MIT. The proposed AGPL/Apache split is an open
owner decision documented in ADR-011, including contributor and App Store
implications.

## Product risks

The dominant risks are sync ambiguity, metadata/object inconsistency, untested
restore, platform background restrictions, E2EE incompatibility with server
features, AI privacy leakage, and roadmap scope. The architecture manages these
with explicit invariants, immutable versions, conservative conflict handling,
separate backup semantics, asynchronous optional services, staged gates, and
documented `OPEN DECISION` records.
