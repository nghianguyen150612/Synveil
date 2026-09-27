# Prompt117 release documentation manifest

This manifest records the intentional Prompt117 documentation scope. It is an
engineering traceability record, not a user installation manual.

## Changed paths

- `README.md` — concise v0.1 support, start-here links, and component summary.
- `docs/README.md` — bilingual release-first navigation and historical-doc
  boundary.
- `docs/en/RELEASE_OPERATIONS.md` — English install, first run, operation,
  recovery, backup/restore, upgrade, purge, security, networking, diagnostics,
  troubleshooting, support matrix, and limitations.
- `docs/vi/RELEASE_OPERATIONS.md` — semantically equivalent Vietnamese guide.
- `docs/en/RELEASE_NOTES_v0.1.md` — user-visible v0.1 capabilities and limits.
- `docs/vi/RELEASE_NOTES_v0.1.md` — semantically equivalent Vietnamese notes.
- `docs/en/SECURITY.md` and `docs/vi/SECURITY.md` — release-status clarification
  while retaining historical threat-model evidence.
- `docs/en/DEPLOYMENT.md` and `docs/en/DESKTOP_LAUNCH.md` — corrected ADR
  relative links.
- `scripts/validate-docs.sh` — deterministic DOC-UNIT-1 through DOC-UNIT-6
  checks for links, bilingual navigation, repository paths, support claims,
  validation wording, and release-facing path/secret hygiene.

## Explicit exclusions

- No Rust/runtime feature, API, migration, installer, deployment architecture,
  or package layout is changed.
- No generated package, `target/` output, log, temporary link report, secret,
  credential, or developer-absolute path is staged.
- Historical Prompt reports and architecture blueprints remain available but are
  not primary user/operator manuals.

## Frozen release facts audited

- Workspace/product version: `0.1.0`.
- Server migrations: `36`.
- Client migrations: `7`.
- `LOCAL_SCHEMA_VERSION`: `7`.
- Supported release-facing platforms: Linux server/client and Linux/Windows
  desktop paths; macOS, mobile, and Synveil OS are deferred.
