# Synveil v0.2 installer common engine

Status: **Implemented by Prompt007**. The implementation is the
`synveil-install-engine` crate in `crates/install-engine`.

## Boundary and public API

The crate exports `InstallerEngine`, `InstallationRequest`, `InstallationPlan`,
`InstallationEffect`, `InstallationAdapter`, `InstallationResult`, and
`EngineEvent`, plus their closed supporting enums. It is platform-neutral and
contains no subprocess, filesystem installer, downloader, Qt, PostgreSQL,
SecretStore, client-sync, first-run, or server-bootstrap implementation.

The engine owns ordering, validation, policy, lifecycle, classification, and
uncertainty handling. An adapter owns typed platform inspection and mechanics:
precondition inspection, privilege availability, apply, effect verification,
uncertain-outcome reconciliation, explicitly supported compensation and its
verification, and final installation verification. There is deliberately no
generic command execution operation.

## Lifecycle and events

```text
Preflight → Plan → Install → Integrate → Verify → Complete
```

Preflight and Plan cannot invoke mutation. A Verify intent has no mutation
effects. All Install effects must precede Integrate effects. Complete can be
entered only after every effect and the final installation contract verify.
Deterministic, in-memory events identify the plan, stable effect, stage,
sequence, and classification without timestamps, random identifiers, secrets,
or raw stderr. P008 can journal these events later; Prompt007 writes nothing.

## Request, plan, and effect contract

Schema version 1 plans bind intent and target scope, ordered effects, explicit
preservation set, final verification, and optional artifact evidence. Stable
semantic IDs contain lowercase ASCII letters, digits, dots, and hyphens.

Every effect declares phase, canonical owner and resource class, adapter
authority, target scope, one or more preconditions, privilege
(`CURRENT_USER`, `NATIVE_AUTHORIZATION`, or `ELEVATED_SYSTEM`), mutation kind,
verification, retry and reconciliation policy, reversibility, safe inverse when
reversible, preservation constraints, and optional exact artifact identity.
Effects are declarative; they never carry executable shell text.

Unknown serialized enums and fields fail closed. `InstallationPlan::from_json`
also rejects unsupported schema versions. The whole plan is validated before
the first adapter apply call: request bindings, identity uniqueness and syntax,
phase order, verification/preconditions, ownership, authority, preservation,
reversibility, reconciliation, and artifact identity must all agree.

## Ownership and preservation

The exact serialized taxonomy is `PACKAGE_OWNED`, `NATIVE_PACKAGE_STATE`,
`PLATFORM_INTEGRATION_OWNED`, `APPLICATION_CONFIG`, `CREDENTIAL_STATE`,
`CLIENT_SYNC_STATE`, `USER_LIBRARY_CONTENT`, `SERVER_CONFIG`,
`SERVER_DATABASE`, `SERVER_OBJECT_DATA`, `EXTERNAL_DEPENDENCY`, and
`EPHEMERAL_RUNTIME_STATE`.

Prompt007 refuses mutation of application configuration, credentials, client
sync state, user libraries, server databases/object data, and external
dependencies. A declared package owner cannot disguise a preserved resource:
the separate resource class is checked against both plan and effect
preservation obligations. Native package state requires native-package-manager
authority; platform integration requires its dedicated adapter authority.

## Execution, uncertainty, and compensation

Immediately before each effect the engine checks privilege and asks the adapter
to re-inspect preconditions. Drift stops as `PLAN_STALE`; the engine neither
repairs nor silently replans. Success and no-op returns both require effect
verification. Failed verification is a known post-mutation partial state.

`FAILURE_BEFORE_MUTATION`, `KNOWN_PARTIAL_MUTATION`, and `OUTCOME_UNKNOWN` stay
distinct. Unknown outcome invokes reconciliation exactly once without replay:
verified-applied proceeds only through normal verification; verified-not-applied
stops with a safe replan disposition; unresolved stops with uncertainty.

After a later failure, earlier effects are considered in reverse order. Only an
effect declared `REVERSIBLE`, carrying a safe inverse, and confirmed supported
by the adapter is compensated. Compensation must itself verify. Partially
reversible, irreversible, native database, and protected durable-state effects
never imply global rollback. Failure is reported truthfully, never as pristine.

## Completion and handoffs

Final verification covers only installer-owned payload, required integration,
and launch eligibility. A completed result sets `launch_eligible`, while
`first_run_complete`, `sync_ready`, and `server_ready` remain false.

- **P006:** authenticates and downloads release bytes. Its non-secret evidence
  is handed over as `VerifiedArtifactEvidence`; this engine authenticates no bytes.
- **P008:** persists events/checkpoints and defines restart recovery.
- **P009:** defines detailed upgrade, repair, uninstall, and purge policy.
- **P010:** maps internal codes to localized user-facing errors.
- **P012+/P021+:** implement native adapters and platform mechanics.
- **First-run/server/sync:** remain separate; the installer creates no profile,
  credentials, library, PostgreSQL instance, host service, or sync activity.
