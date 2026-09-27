# v0.2 clean-machine acceptance contract

Status: **Prompt004 contract defined; native execution pending.** This document
and the versioned scenarios define future evidence. A definition or successful
validation of that definition does not mean product behavior passed.

## Cross-document contract audit

P001 and ADR-049 set Windows Setup, DEB, RPM and AppImage as primary desktop
targets, with a verified Linux installer as an alternate; P002 records which
of those v0.2 implementations and native journeys are absent. Installation
completion is payload, integration and launch verification. Connect and Host
first-run milestones remain separate, `synveil-client` owns synchronization,
and OS SecretStore owns durable credentials. ADR-050 and P003 require
data-preserving ordinary uninstall, consent for startup, scoped repair,
reconciliation before retrying an unknown effect, fail-closed unknown schema,
and a separate Host bootstrap boundary. Platform privilege expectations remain
per-user Windows, visible native DEB/RPM package authorization and user-level
AppImage. Native owners and journey IDs match the unchanged roadmap.

## Scope and clean-machine meaning

The authoritative scenario records live in
[`tests/install-acceptance/scenarios/`](../../tests/install-acceptance/scenarios/)
and follow schema version 1 in
[`scenario-v1.schema.json`](../../tests/install-acceptance/schema/scenario-v1.schema.json)
and result records use
[`result-v1.schema.json`](../../tests/install-acceptance/schema/result-v1.schema.json).
The validator in `scripts/install_acceptance.py` is the normative closed-enum
and cross-field checker. JSON is used so the contract needs no new parser
dependency.

A clean machine has no Synveil installation, package registration,
installer-owned integration, prior client profile (except where a scenario
requires an upgrade/recovery source), or stale acceptance state. It need not be
a fresh OS and must retain unrelated applications and user files.

Scenarios declare platform family, pending or qualified OS versions,
architecture, desktop/package-manager facts, required capabilities, artifact
identity fields, ordered typed product steps, observable assertions,
preservation snapshots, forbidden outcomes, cleanup ownership, timeout policy,
and future roadmap owners. There is no arbitrary command or shell field.
Cleanup is best-effort and limited to scenario-created resources; cleanup
failure is recorded independently.

## Execution, capability and result semantics

Capabilities are required explicitly. The runner records each as `available`,
`unavailable`, or `unknown`; checks it can establish locally are recorded and
external/service-dependent checks remain `unknown`. Conditional capability
requirements apply only to their declared platform families. Unavailable or unknown
required capabilities block an applicable scenario. An intentionally
inapplicable platform is `SKIPPED`. Missing implementation or artifact is
`BLOCKED`; environment-valid behavior violating an assertion is `FAIL`;
harness malfunction or unsafe definition is `ERROR`; `PASS` requires all
assertions at the declared evidence level. Validate-only mode never executes
product steps. `run <id>` reports the detected host/capability facts and emits
`BLOCKED` or platform-inapplicable `SKIPPED` until a native execution adapter
and qualified implementation are available.

Every result record has schema version, scenario digest, runner version,
timestamps (nullable until execution), platform facts, artifact identity,
finite result, completed steps, assertion outcomes, evidence class, blocker
reason, diagnostic references, and independent cleanup status. Secret values
are prohibited and result records mark diagnostics redacted; credential checks
use SecretStore identity/presence/usability, not secret bytes.

## Evidence ladder and release interpretation

Evidence strength is ordered:

1. `contract-valid`
2. `static`
3. `fixture`
4. `ci-native-scoped`
5. `native-clean-machine`
6. `live-external-dependency`
7. `interruption-power-cycle`
8. `release-acceptance`

A scenario declares its minimum evidence. Weaker evidence cannot satisfy a
stronger minimum. Static, fixture, cross-build and offscreen results cannot
prove native clean-machine behavior; process-termination fixtures do not prove
physical or VM interruption acceptance. Release acceptance must retain
scenario digest, runner/source commit, artifact digest, OS/version,
architecture, machine or CI identity and execution time.

## Preservation and product boundaries

Preservation is explicit for application config, credential state, client sync
state, user library content, server config, server database and server object
data. Stable logical fingerprints are preferred where timestamps or irrelevant
metadata can vary. Ordinary uninstall preserves durable user/server state;
destructive purge is outside the ordinary-uninstall scenario. Repair targets
package-owned state only. Upgrade and repair preserve explicit startup
preference. Credentials remain in the OS SecretStore. Desktop/client control
remains local and user-scoped; no public control listener is an allowed
dependency.

Installation completion means verified payload, platform integration and a
supported launch path. It does not imply authentication, server reachability,
library setup, sync completion or Host readiness. Connect assumes a reachable
test server as a precondition; Host provisioning remains behavior under test
and does not freeze unresolved PostgreSQL or service implementation choices.
An ambiguous mutation outcome requires authoritative reconciliation before
retry. Unknown/newer database schema fails closed.

Stable progress milestones are `INSTALLATION_READY`, `APPLICATION_LAUNCHED`,
`PROFILE_READY`, `AUTHENTICATED`, `LIBRARY_READY`, `SYNC_STARTED`, and
`SERVER_READY`. Scenarios declare only the milestones they exercise.
Interrupted-install acceptance names acquisition, package-payload mutation,
platform-integration and verification boundaries, and distinguishes known
pre-mutation failure, known partial mutation and `OutcomeUnknown` recovery.

The machine-readable scenarios preserve the P001 journey identifiers. They
cover Windows Setup, DEB, RPM, AppImage, quick install, repair, ordinary
uninstall, interrupted recovery, Connect and Host first run. An additional
`UPGRADE-TEMPLATE-1` represents supported source metadata, including the v0.1.0
commit, without asserting a supported upgrade matrix. Native execution belongs
to the implementation and acceptance prompts recorded in each scenario.

Run the non-mutating validator and focused self-tests with:

```bash
./scripts/validate-install-acceptance.sh
```

Use `python3 scripts/install_acceptance.py inventory` for a deterministic
machine-readable scenario inventory. Validation proves only contract
integrity and inventory completeness.
