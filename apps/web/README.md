# Synveil Web

This directory contains the React, strict TypeScript, and Vite browser boundary
described by ADR-009. It is intentionally only an application foundation:
product authentication, files, uploads, backups, sync, and real health data
remain unavailable until their API contracts and implementation gates pass.

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

The initial routes are `/`, `/health/dev`, and a semantic not-found page. The
health route is an explicit development placeholder and does not claim that
production health has been checked.
