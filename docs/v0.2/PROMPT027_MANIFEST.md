# Prompt027 evidence manifest — Windows lifecycle

## Identity and inherited state

* Starting SHA: `fbffe79d0cac22ceee439d93bb5ba1d9364d40e9`.
* Branch: `codex/p027-windows-lifecycle`.
* P026 inherited status: source/static implemented; final-head hosted native
  evidence pending. This checkout has no configured `origin`; PR #41, #42, and
  #43 final heads and workflows could not be inspected. No inherited result was
  promoted from a summary.
* Direct inherited fixes: none classified locally. No observed P027 work was
  redirected into unrelated Linux/PostgreSQL failures.

## Implemented results

* Repair: exact AppId/location, trusted manifest, same strict stable version,
  and `/REPAIR=1` required in silent mode. Missing/malformed/conflicting identity
  fails before mutation. Package bytes and reviewed integration are restored;
  all durable state and observed startup/desktop choices remain preserved.
* Upgrade fixture: CI-only numeric `1.0.0` to `1.1.0` real Setup execution over
  disposable runtime copies. Cargo.toml, tags, production version authority,
  and P005 release metadata remain untouched.
* Downgrade: newer installed version rejects before mutation, including silent
  execution. Unknown/non-stable installed versions fail closed.
* Obsolete ownership: only trusted old-manifest minus target-manifest regular
  files beneath the canonical package root qualify. Traversal/reparse ambiguity
  rejects; unknown adjacent files remain preserved.
* Startup: repair/upgrade preserve preference unless a current explicit choice
  is supplied. Uninstall synchronously calls the P026 client-owned cleanup
  boundary before binary removal and preserves the preference file.
* Ordinary uninstall: registered HKCU command is discovered rather than an
  uninstaller filename being assumed. Package/integration is removed while
  application config, credentials, sync state, user libraries, all server data,
  and external state remain preserved. Unknown adjacent files may keep the
  package directory present. Reinstall is exercised.
* Purge: remains separate, explicit, confirmed, and limited in the generic
  model to APPLICATION_CONFIG. Credentials, client state, user libraries, and
  server data are not representable targets.

## Evidence status

Local source/static/unit evidence: implemented. Native standard-user script and
bounded JSON evidence schema: implemented, but not run on this Linux host.
Hosted workflow ID, final artifact SHA, native repair/upgrade/downgrade/
uninstall/reinstall result: **not available / pending**, because no remote is
configured and Windows/Inno/Task Scheduler are unavailable locally. The
workflow will carry P024 runtime, P025 standard-user, and P026 startup checks
alongside the P027 lifecycle fixture.

Known blocker: native Windows execution and hosted workflow access. P028 still
owns interactive native installation, GUI/named-pipe/login execution, final
lifecycle matrix, and the readiness checkpoint. No P028 completion or Windows
experience readiness claim is made.

## Files changed

* `.github/workflows/windows-installer.yml`
* `deploy/windows/installer/Synveil.iss`
* `scripts/build-windows-installer.ps1`
* `scripts/invoke-windows-standard-user-test.ps1`
* `scripts/test-windows-installer-lifecycle.ps1`
* `scripts/validate-windows-installer.py`
* `scripts/windows_lifecycle.py`
* `scripts/test_windows_lifecycle.py`
* `docs/v0.2/WINDOWS_LIFECYCLE.md`
* `docs/v0.2/PROMPT027_MANIFEST.md`
* `docs/v0.2/ROADMAP.md`
