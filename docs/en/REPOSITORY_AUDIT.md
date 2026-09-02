# Repository audit

- Status: **Accepted baseline**
- Audit date: 2026-08-21
- Audited revision: `5f70a0b` on `main`

## Executive finding

Synveil begins as a clean, architecture-first greenfield repository. At the
audited revision it contains exactly two tracked files: `README.md` and
`LICENSE`. There is no product implementation to refactor, no persisted data
format to migrate, and no existing API contract to preserve.

This finding authorizes documentation and deliberate Phase 0 scaffolding; it
does not authorize product claims.

## Existing material

| File | Finding | Disposition |
|---|---|---|
| `README.md` | Two-line placeholder. Its capability sentence is truncated at `organizatio` and does not distinguish plans from implementation. | Replace with a status-honest blueprint index. |
| `LICENSE` | MIT License, copyright 2026 Nguyen Nghia. | Preserve unchanged. Treat any proposed license change as a legal/owner decision. |

The repository was clean before this blueprint work, tracked `origin/main`, and
had one commit (`Initial commit`). There were no hidden source directories,
untracked implementation files, or repository-local agent instructions.

## Reusable elements

- The project name, **Synveil**, is established.
- The placeholder describes the intended self-hosted combination of storage,
  backup, synchronization, photos, code, and AI organization.
- The repository and remote provide a clean place to establish the monorepo.

There are no reusable components, schemas, migrations, APIs, tests, containers,
or deployment manifests.

## Conflicts and assumptions

### License direction

The current repository is MIT licensed, while the product brief prefers
AGPL-3.0 for core/server/web and Apache-2.0 where an integration SDK benefits
from permissive adoption. The blueprint cannot silently change a legal grant.
ADR-011 therefore remains `Proposed`, and `LICENSE` remains unchanged.

### Implementation status

The old README could be read as saying the catalogue already exists. It does
not. The replacement uses the project-wide status taxonomy and marks the
repository as pre-implementation.

### Technical direction

There is no instantiated stack to conflict with the preferred Rust, React,
PostgreSQL, Python, Docker Compose, and Caddy direction. All diagrams and
modules in this documentation are target designs.

## Missing infrastructure

The audit confirmed the absence of:

- Rust workspace, crates, source, formatting, linting, and dependency policy;
- JavaScript package/workspace metadata and React/Vite application;
- Python environment or AI service;
- PostgreSQL schema and migrations;
- storage adapter, OpenAPI file, domain code, sync/backup protocols, and jobs;
- Compose, Dockerfiles, Caddy configuration, sample configuration, and secret
  handling;
- CI, tests, fixtures, benchmarks, fuzz targets, and recovery harnesses;
- contribution, security, governance, release, and upgrade procedures;
- English/Vietnamese specifications and ADRs.

These are roadmap inputs, not defects in an existing implementation.

## Migration recommendation

Treat Phase 0 as greenfield scaffolding. Add only directories that a gated task
uses. Do not manufacture compatibility requirements for code that does not
exist, and do not implement feature modules during the blueprint phase.

Once persistent data exists, this greenfield assumption expires. All future
schema and storage-format changes must follow the forward migration and
compatibility rules in `DEPLOYMENT.md` and accepted ADRs.

## Audit evidence and reproducibility

The audit used read-only repository inspection: Git status/history/tree,
complete file enumeration excluding `.git`, file content inspection, and Git
object integrity checking. Any later implementation task must repeat at least
the status and file-inventory checks because this document is a point-in-time
baseline, not a live assertion.
