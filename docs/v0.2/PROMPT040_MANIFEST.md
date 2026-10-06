# Prompt040 manifest — Library First-Run Wizard

## Baseline and scope

- Roadmap allocation: Phase F, P040 (between P039 Authentication UX Polish
  and P041 Installation-to-Sync Progress Experience).
- Expected baseline: `0c0032329ee9791fff12b3d115501caf63a2f581`.
- Actual starting `origin/main`: `0c0032329ee9791fff12b3d115501caf63a2f581`.
- Prompt039 merge commit is present in `origin/main`; no commits intervened.
- Repository: `/workspace/Synveil`.
- Branch: `codex/p040-library-first-run-wizard`, created from that clean main
  baseline.
- Scope: one-page first-library desktop onboarding, authoritative routing,
  bounded setup result presentation, focused tests, static validation, docs,
  and focused CI. P041/P042 are deferred.

## Existing architecture audited

The existing operation is reused:

```text
QML → DesktopUiBridge → DesktopController → local authenticated IPC
    → synveil-client / DesktopControlHandle → authenticated library API
    → durable local root binding → DesktopSyncHost / SyncRuntime
```

The audit covered the requested P037–P039 documentation, ADR-044/045/048,
ADR-066/069, `docs/en/DESKTOP_CONTROL.md`, the desktop bridge/presentation/QML,
the client controller/control path, client-sync HTTP library creation,
existing-root validation and observation, and focused CI/test scripts.

ADR-045 already admits ordinary non-empty existing folders for a newly created
remote library. Existing entries remain in place and enter the existing bounded
observation/sync pipeline. Root safety, `.synveil` marker identity, profile and
device scope, overlap checks, pending identity, reconciliation, durable binding,
and runtime ordering remain in client/client-sync ownership. Local paths remain
local and are absent from server library metadata. Existing-remote attach/import
is unsupported and unchanged.

## Implementation

- The first-library route is a fresh client snapshot with configured profile,
  authenticated device, and zero configured libraries. Welcome, connection
  setup, and sign-in continue to take precedence. Existing libraries bypass
  P040; no GUI completion flag was added.
- The focused page asks only for a library name and native folder selection.
  Existing ordinary files are described as staying in place. QML does no
  recursive scan or filesystem validation.
- The existing setup command and process/controller admission remain the only
  mutation path. Rust presentation maps canonical results to a finite code,
  safe message, and next-action code.
- Success and `OutcomeUnknown` remain in an indeterminate checking state until
  a newer fresh snapshot arrives. The page exits only when authoritative state
  shows a configured library. An unresolved setup keeps the same-folder resume
  path; no remote create is automatically replayed.
- The selected path remains transient presentation input. The native picker
  cancellation performs no setup request. Inputs/actions are disabled while
  admitted; accessible names and stable object names cover the page, controls,
  status, and feedback.
- No P041 first-sync progress, P042 repair flow, Phase-F marker, server API,
  migration, new sync owner, or existing-remote attach path was added.

## Files

Added:

- `.github/workflows/library-first-run-wizard.yml`
- `docs/adr/ADR-070-v0.2-library-first-run-wizard.md`
- `docs/v0.2/LIBRARY_FIRST_RUN_WIZARD.md`
- `docs/v0.2/PROMPT040_MANIFEST.md`
- `scripts/validate-library-first-run-wizard.py`

Modified:

- `crates/desktop/qml/Main.qml`
- `crates/desktop/src/bridge.rs`
- `crates/desktop/src/presentation.rs`
- `docs/adr/README.md`
- `docs/v0.2/ROADMAP.md`
- `scripts/validate-docs.sh`

No client, server, sync, schema, upload, iOS, or Android source was modified.

## Tests and validation performed

Added Rust presentation tests for the authoritative routing matrix, newer
fresh-snapshot confirmation rule, and bounded library setup result mappings.
The network-free validator checks routing, picker behavior, canonical ownership,
path privacy, admission, pending identity reuse, stale success handling, safe
existing-folder behavior, accessibility names, ordinary product wording, and
P041/P042 scope boundaries.

Passed locally:

- `python3 scripts/validate-library-first-run-wizard.py`
- `python3 -m py_compile scripts/validate-library-first-run-wizard.py`
- `./scripts/validate-docs.sh`
- `./scripts/validate-install-acceptance.sh` (contract suite: 21 tests passed;
  native clean-machine execution was not performed)
- `bash -n scripts/validate-docs.sh scripts/test-desktop-ui.sh`
- `git diff --check`

Unavailable locally:

- `cargo fmt`, Rust unit/integration tests, `cargo check`, and Clippy could not
  run because Cargo/Rust are not installed in this Cloud shell.
- `./scripts/test-desktop-ui.sh` stopped at its first Cargo invocation for the
  same reason.
- `qmllint` could not run: the installed wrapper attempted to execute the
  missing `/usr/lib/qt5/bin/qmllint`. No interactive Qt folder-picker
  acceptance was claimed.
- No native desktop build or interactive GUI test ran in this environment.

## Inherited hosted CI context at the starting baseline

Prompt039 PR #68 is merged at the expected baseline. Its focused workflow run
`37386869554` reported `auth-domain-and-controller` and
`auth-security-regressions` passed; `desktop-auth-presentation` failed at
`./scripts/test-desktop-ui.sh`, and `quality` failed at the Clippy command.
GitHub exposed the failed step names but returned no failed-step log text when
queried, so no more specific cause is asserted here. These results predate
P040. Other unrelated workflows on the same main SHA also had failures; they
are not attributed to P040.

## Hosted workflow / Git status

- P040 focused workflow: not observed because no remote branch or PR exists.
- The single consolidated Prompt040 commit has subject
  `feat: add first-run library wizard`; its final SHA is reported with the
  completion status because embedding a commit's own SHA in its tree would
  change that SHA.
- Push: not completed. The authenticated GitHub CLI session and configured
  `gh auth git-credential` helper are present, and the repository API reports
  push permission, but Git HTTPS push returns HTTP 401. `GH_TOKEN` is present
  by variable name, but the Git credential helper does not provide a
  Git-compatible credential accepted by the Git HTTPS endpoint. No credential
  value was printed or substituted. No alternate API-generated commit was
  used.
- PR number/URL: not created because the branch could not be published.
- Hosted P040 CI result: not observed because no PR/remote branch exists.
- Merge result and resulting `origin/main` SHA: not yet applicable.
- Branch protection status: GitHub returned HTTP 403 to the branch-protection
  API request; merge requirements will be determined from the real PR checks
  and merge state.
- The branch remains local and unpublished; the final working tree is clean.

P041 installation-to-sync progress and P042 repair/recovery UX remain deferred.
