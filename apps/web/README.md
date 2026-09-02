# Synveil Web

This directory contains the React, strict TypeScript, and Vite browser boundary
described by ADR-009. The first-run setup page, login page, server-derived auth
state machine, guarded authenticated shell, and logout/CSRF boundary are
implemented for the current bootstrap contract. Protected routes now remain
unmounted through auth bootstrap, preserve only sanitized same-origin return
paths, and use one single-flight session recovery coordinator. Recovery
reconstructs pages from GETs under a fresh auth generation; it never replays a
POST. Typed exact-offset upload API
helpers now send raw `Blob`/`ArrayBuffer` bodies through the same CSRF-aware API
client, but no upload picker, queue, progress UI, or file browser is present.
The authenticated Backup Control Center now provides backup-set overview and
detail routes, bounded snapshot-tree inspection, retention configuration,
explicit manual maintenance, two-phase restore, and guarded two-phase prune
workflows against the accepted backup API contracts. Download, sync, and real
health data remain unavailable until their separate contracts and
implementation gates pass.

## Development

From this directory:

```text
npm install
npm run dev
```

The project keeps development tools available for the documented local quality
commands. A production-only install can explicitly use `npm install --omit=dev`
after the static build has been produced.

Quality checks:

```text
npm run lint
npm run typecheck
npm run test
npm run build
```

The browser communicates through `src/api/client.ts`. `VITE_API_BASE_URL` may
select a public API origin for local development, but browser-visible Vite
variables are never a place for credentials or other secrets. The default API
base is deployment-relative.

The backup routes are `/backups`, `/backups/:backupSetId`, scoped restore and
prune workflow routes, and
`/backups/:backupSetId/operations/:operationKind/:operationId`. Prompt 57
same-tab mutation recovery records use schema v2 and the minimum public user ID
needed to block cross-account replay; legacy or foreign-owner records never
issue requests and may only be discarded under the current account. `/setup`
and `/login` are
guarded by the server bootstrap/session state. The health route remains an
explicit development placeholder and does not claim that production health has
been checked. Backup mutations are deliberate browser actions; this app does
not schedule work, auto-advance maintenance, run physical cleanup, or claim
space reclamation.
