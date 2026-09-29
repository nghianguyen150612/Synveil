# Synveil v0.2 roadmap — Effortless Setup

Status: **Authoritative Prompt001–048 roadmap; implementation targets, not shipped claims**.
Target release: `v0.2.0`. Development starts from released v0.1.0 at
`fa23232ff0154f627ebdd221ec5435134f177af0`.

The north star is:

> Clean machine → installed Synveil → usable first-run experience → syncing files, with no terminal required for normal users.

The [installation product contract](INSTALLATION_PRODUCT_CONTRACT.md) and
[ADR-049](../adr/ADR-049-v0.2-effortless-installation-architecture.md) define
the product and architecture constraints for this roadmap. Every successful
prompt validates its scope, stages explicit reviewed paths, creates one focused
commit, pushes `origin/main`, verifies the live remote and clean worktree, then
reports its gate. Prompt048 is the sole v0.2.0 tag/release gate.

## PHASE A — PRODUCT / INSTALLATION CONTRACT (P001–004)

### P001 — Effortless Installation Product Contract and Blueprint

Freeze installer philosophy, flows, support matrix, architecture and acceptance
criteria. Establishes the [installation product contract](INSTALLATION_PRODUCT_CONTRACT.md),
ADR-049 and the authoritative roadmap. Documentation only; does not implement
an installer or change v0.1 runtime behavior.

### P002 — Existing Installation Surface Audit

Inventory every current package, install script, service, first-run flow,
dependency, manual step and blocker inherited from v0.1. Label source, fixtures,
native platform and environment-gated evidence separately.

**Complete:** [existing installation surface audit](EXISTING_INSTALLATION_SURFACE_AUDIT.md)
and [Prompt002 evidence manifest](PROMPT002_MANIFEST.md). The audit records
the v0.1.0 source baseline and distinguishes CI package/smoke evidence from
clean-machine GUI installation acceptance.

### P003 — Installation Architecture and Component Ownership

Define installer engine, platform adapters, package ownership, server bootstrap
boundary, rollback/repair model and privilege model. Reconcile existing data,
credential, process and migration owners before implementation.

**Complete:** [installation architecture and component ownership](INSTALLATION_ARCHITECTURE.md),
[ADR-050](../adr/ADR-050-v0.2-installation-component-ownership.md), and the
[Prompt003 manifest](PROMPT003_MANIFEST.md). Architecture only; no installer,
server bootstrap, schema, artifact, runtime or v0.1 behavior was implemented.

### P004 — Clean-Machine Acceptance Harness

Create deterministic acceptance definitions and machine-readable scenarios for
Windows/Linux install, first run, upgrade, repair and uninstall. Record exact
platform/version/artifact preconditions and assertions; do not claim native
acceptance from fixtures alone.

**Complete:** [clean-machine acceptance contract](CLEAN_MACHINE_ACCEPTANCE.md),
versioned scenarios and validator, and the [Prompt004 manifest](PROMPT004_MANIFEST.md).
The contract is defined; native execution remains pending under its named
acceptance owners.

Phase A checkpoint: **V0.2 INSTALLATION FOUNDATION LOCKED** — the P001–P004
product, audit, ownership and acceptance contracts are consistent and frozen
for Phase B. This does not claim that installer journeys are implemented.

## PHASE B — RELEASE DISTRIBUTION FOUNDATION (P005–011)

### P005 — Unified Release Artifact Manifest

Define one authoritative artifact metadata format for DEB, RPM, AppImage,
Windows installer and retained portable artifacts.

**Complete (Prompt005):** schema v1, deterministic standard-library tooling,
current DEB/RPM/portable-ZIP producer integration, composition, safety tests,
and CI validation are defined in the [release artifact manifest contract](RELEASE_ARTIFACT_MANIFEST.md)
and [Prompt005 evidence manifest](PROMPT005_MANIFEST.md). AppImage and Windows
installer are schema-supported but remain unimplemented. Phase B remains open.

### P006 — Release Download and Integrity Contract

**Complete (Prompt006):** exact-manifest pin authentication, signature-ready
fail-closed trust boundaries, HTTPS/origin/redirect policy, deterministic exact
artifact selection, and bounded atomic verification are defined in the
[release download contract](RELEASE_DOWNLOAD_INTEGRITY.md) and
[Prompt006 evidence](PROMPT006_MANIFEST.md). Production signing and P011
channel/version recommendation remain pending. Phase B remains open.

### P007 — Installer Common Engine

**Complete (Prompt007):** the `synveil-install-engine` workspace crate implements
the shared, platform-neutral coordinator documented in the
[installer common engine contract](INSTALLER_COMMON_ENGINE.md), with 81 focused
fake-adapter tests and [Prompt007 evidence](PROMPT007_MANIFEST.md):

```text
Preflight → Plan → Install → Integrate → Verify → Complete
```

It validates the complete plan before mutation, preserves native package and
durable-state ownership, rechecks preconditions, verifies every mutation,
reconciles uncertain outcomes without replay, and requires final verification.
P009–P011 remain pending; Phase B remains open.

### P008 — Installation Transaction Journal

**Complete (Prompt008):** the [installation transaction journal](INSTALLATION_TRANSACTION_JOURNAL.md)
provides exact-plan binding, append-only durable checkpoints, exclusive writer
semantics, fail-closed restart detection, and safe recovery. An ambiguous
mutation is reconciled and never blindly replayed. Evidence is fixture and
process-interruption simulation, not native physical power-loss acceptance.

### P009 — Upgrade / Repair / Uninstall Common Contract

**Complete (Prompt009):** the [shared lifecycle contract](UPGRADE_REPAIR_UNINSTALL_CONTRACT.md) defines exact upgrade compatibility, same-version repair, data-preserving ordinary uninstall, and a separate fail-closed purge boundary. Native acceptance remains pending.

### P010 — Installer Error Model

**Complete (Prompt010):** the [installer error model](INSTALLER_ERROR_MODEL.md)
maps P006 acquisition, P007 engine, P008 journal/recovery, and P009 lifecycle
failures into finite user-facing categories, safe actions, explicit retry
semantics, and bounded typed diagnostics. Unknown outcomes require
reconciliation before retry; integrity failures expose no bypass. P011 remains
pending and Phase B is still open.

### P011 — Release Channel and Version Selection

**Complete (Prompt011):** the [release channel contract](RELEASE_CHANNEL_SELECTION.md) and [Prompt011 evidence](PROMPT011_MANIFEST.md) define authenticated stable-only metadata, generation/high-water rollback protection, deterministic highest compatible selection, and exact P006 manifest binding. P009 remains mutation authority; no automatic updater is introduced.

Checkpoint achieved: **V0.2 DISTRIBUTION CORE READY** — P005–P011 common distribution contracts and engines are ready. This does not claim Linux easy install, Windows Setup, AppImage, server setup, or a v0.2.0 release.

## PHASE C — LINUX EASY INSTALL (P012–020)

### P012 — Linux Package Integration Reconciliation

**Complete (Prompt012):** the [Linux package integration reconciliation](LINUX_PACKAGE_INTEGRATION_RECONCILIATION.md) and [Prompt012 evidence](PROMPT012_MANIFEST.md) unify current DEB/RPM lifecycle with installer-common semantics while retaining native manager ownership, safe hooks and data-preserving package removal. P013–P020 and the Phase C checkpoint remain pending.

### P013 — Debian/Ubuntu Desktop Package UX

Deliver and qualify double-click DEB installation, desktop entry, runtime
dependencies and first launch on each advertised Debian/Ubuntu environment.

### P014 — Fedora/RPM Desktop Package UX

Deliver and qualify the equivalent native RPM experience on each advertised
Fedora/RPM environment.

### P015 — AppImage Build Foundation

Produce a self-contained supported AppImage without breaking SecretStore or
IPC semantics. Define the qualified runtime baseline and fail-closed behavior.

### P016 — AppImage Runtime Integration

Define desktop integration, icons, safe writable state and background-client
strategy for the portable AppImage lifecycle.

### P017 — Linux Quick Install Script

Implement the supported installer script using `curl ... | sh` or an explicitly
documented safer equivalent. Verify release identity and payload before
privileged package changes.

### P018 — Linux Distro and Architecture Detection

Detect package family, architecture and version compatibility. Choose an
artifact only when the environment is qualified; reject unsupported systems.

### P019 — Linux First Launch Integration

Deliver Install → Open Synveil without requiring ordinary users to enter
manual systemd commands. Preserve explicit autostart choices.

### P020 — Linux Clean-Machine Acceptance

Validate DEB, RPM, AppImage and script paths on disposable supported native
environments, including launch, repair, upgrade, data-preserving uninstall and
interruption recovery as applicable.

Checkpoint: **V0.2 LINUX INSTALL EXPERIENCE READY**

## PHASE D — WINDOWS SETUP EXPERIENCE (P021–028)

### P021 — Windows Installer Technology Decision

Choose and lock installer technology based on current architecture,
maintainability, per-user installation and unattended-test support.

### P022 — SynveilSetup.exe Skeleton

Create the real Windows installer executable project and build path.

### P023 — Windows Installer UI

Implement the concise native flow:

```text
Welcome → Terms/options → Install → Finish
```

### P024 — Windows Runtime Deployment

Install desktop, client and Qt runtime correctly without requiring manual ZIP
extraction.

### P025 — Windows Per-User Installation

Support normal per-user installation without administrator access where
feasible; justify and disclose any privileged operation.

### P026 — Windows Startup Integration

Provide the friendly choice **Start Synveil when I sign in**, backed by the
existing least-privilege current-user Task Scheduler implementation.

### P027 — Windows Repair / Upgrade / Uninstall

Preserve Synveil state while repairing or replacing package-owned files. Keep
ordinary uninstall data-preserving and provide a separate explicit destructive
data-removal choice only under its own reviewed contract.

### P028 — Windows Native Acceptance CI

Run the installer on native Windows CI, launch the actual installed app, and
validate named-pipe and Task Scheduler behavior. Cross-build evidence alone
does not pass this gate.

Checkpoint: **V0.2 WINDOWS INSTALL EXPERIENCE READY**

## PHASE E — GUIDED SERVER SETUP (P029–036)

### P029 — Server Setup Product Contract

Define normal and Advanced server setup. The primary product choice is:

```text
Host Synveil on this device
or
Connect to existing Synveil
```

### P030 — Server Dependency Strategy

Decide supported automatic database/runtime provisioning without exposing
PostgreSQL complexity to ordinary users. Preserve PostgreSQL as the canonical
server metadata authority.

### P031 — Managed Server Configuration

Generate safe configuration, credentials and directories from user-friendly
choices, preserving secret ownership, permissions and recovery boundaries.

### P032 — Storage Location Wizard

Let the user select where Synveil stores data; validate capacity, path and
ownership safely. Distinguish server storage from the user's synchronized
folder.

### P033 — Server Service Installation

Automatically configure approved service supervision with a defined privilege
boundary, startup policy, repair and data-preserving removal.

### P034 — Server Network and Reachability Setup

Guide local, LAN and approved remote reachability without requiring ordinary
users to understand raw bind configuration. Do not silently expose public
listeners or add a mandatory proprietary relay.

### P035 — Server First-Admin Bootstrap

Deliver secure guided initial administrator creation with protected credentials,
safe recovery and explicit readiness.

### P036 — End-to-End Self-Host Wizard

Validate the clean supported server journey:

```text
Install → Host this device → choose storage → admin setup → server ready
```

Checkpoint: **V0.2 GUIDED SELF-HOSTING READY**

## PHASE F — FIRST-RUN PRODUCT UX (P037–042)

### P037 — Unified Welcome Experience

On first launch, offer **Host Synveil** or **Connect to Synveil** in product
language without engineering terminology.

### P038 — Connection Setup Simplification

Reduce server address and profile setup to ordinary user concepts while
preserving existing origin validation and connection ownership.

### P039 — Authentication UX Polish

Improve user-facing sign-in, error and recovery states without changing the
authentication protocol or credential owner.

### P040 — Library First-Run Wizard

Guide creation of the first library and selection of its local folder with
minimal screens and safe existing-folder handling.

### P041 — Installation-to-Sync Progress Experience

Show truthful, meaningful progress from installation through setup and first
synchronization, distinguishing completed stages and user action required.

### P042 — Repair and Recovery UX

Provide user-facing **Repair Synveil**, **Reconnect server**, **Restore missing
folder** and **Restart background service** actions where supported, without
exposing internal mechanisms. Missing local roots never imply remote deletion;
unknown outcomes are reconciled before retry.

Checkpoint: **V0.2 FIRST RUN EXPERIENCE READY**

## PHASE G — RELEASE PRODUCTIZATION (P043–048)

### P043 — Installer Security Hardening

Harden privilege boundaries, path handling, download integrity, installer
injection defenses and downgrade safety.

### P044 — Installation Resilience Hardening

Exercise power loss, interrupted upgrades, partial installs, resumability and
disk-full behavior while preserving user data.

### P045 — Cross-Platform Clean-Machine Matrix

Run fresh Windows, Debian, Fedora and generic Linux acceptance on the exact
platform versions advertised for v0.2.

### P046 — v0.2 Documentation and Distribution Readiness

Prepare concise user-facing installation documentation because the installer
handles ordinary setup complexity. Keep unsupported cases and Advanced guidance
clear.

### P047 — v0.2 Final Release Candidate Validation

Run full product, installation, server, first-run and package acceptance.
Record exact artifact identities and distinguish native, fixture and unavailable
evidence.

### P048 — Synveil v0.2.0 Release Gate

Freeze the exact source and final artifacts, create and verify the `v0.2.0` tag,
and verify remote release state. This is the only roadmap prompt that may create
the final v0.2.0 release tag after all required gates pass.

Final gate: **SYNVEIL_V0_2_0_RELEASED**

## Release acceptance retained by the roadmap

| Surface | Required user journey |
| --- | --- |
| Windows | Download `SynveilSetup.exe` → double click → accept terms → choose options → Install → Finish → Synveil opens. |
| Debian / Ubuntu | Download `.deb` → double click → Install → Open. |
| Fedora / RPM | Download `.rpm` → Install → Open. |
| Generic Linux | Download AppImage → open. |
| Terminal alternate | One supported install command. |
| Self-hosting | Offer **Host Synveil on this device**; do not require ordinary users to configure PostgreSQL, `DATABASE_URL` or service files manually. |

## Product quality bar

Preserve simplicity, clarity, safe defaults, minimal decisions, native
conventions, reversibility, data preservation and progressive disclosure.

> The easiest way to install Synveil should also be the recommended way to install Synveil.

## Architectural decisions and their deadlines

These existing architectural decision gates remain subordinate to the exact
prompt ownership above. They do not move or merge prompts.

| Decision | Owner | Required prompt / constraint |
| --- | --- | --- |
| Exact supported OS versions, Linux compatibility and GUI execution baseline | Platform / Distribution, Release | P002 audits the current surface; P003 assigns ownership; P018/P020 qualify Linux; P028/P045 qualify Windows and all advertised platforms before support claims. x86_64 only in the initial matrix unless a later decision adds evidence. |
| Release signing, bootstrap-script trust and key rotation | Release, Security | P005 defines manifest; P006 closes integrity and trust before downloaded payloads are executed; P017 consumes the Linux script boundary; P043 hardens it. |
| Shared engine, durable journal, repair and typed error boundaries | Distribution, Desktop, Security | P007 defines engine; P008 interruption journal; P009 lifecycle; P010 error categories. Their phase-B checkpoint is P011. |
| Windows installer technology and update/repair strategy | Distribution, Desktop, Security | P021 selects technology; P022–027 implement the installer lifecycle; P028 supplies native evidence. |
| Managed PostgreSQL distribution (`OD-PLAT-001`, proposed ADR-019) | Database, Release, Security, Product | P029 defines setup modes; P030 decides provisioning before implementation; preserve PostgreSQL and do not add SQLite as a production server authority. |
| Host supervisor/elevation (`OD-PLAT-002`) | Platform, Release, Security | P030 decides dependency strategy; P033 closes service privilege/supervision before server service implementation. Existing desktop client supervision remains under ADR-041. |
| Offered access modes (`OD-PLAT-003`, proposed ADR-020) | Networking, Security, Product | P029 defines product choices; P034 closes offered reachability before new remote-access mechanisms. No silent public exposure or mandatory proprietary relay. |
| Supported upgrade sources and rollback/restore limits | Database, Client, Release | P009 defines lifecycle; P027 applies it on Windows; P043/P044 harden it; P047 validates supported sources and coordinated restore before P048. |
| Exact diagnostic limits and product copy | Desktop, Product, Security, Accessibility | P010 bounds error categories; P037–042 apply them in first-run/recovery; P043 hardens redaction. |

The open platform decisions remain in [PLATFORM.md](../en/PLATFORM.md).
Prompt001 fixes the installation-versus-first-run product boundary and primary
artifact policy. It does not silently accept proposed database/remote-access
ADRs or select a new privileged host supervisor.

## Required evidence and promotion rules

- P004 defines deterministic acceptance scenarios for the release journeys
  retained above; P020/P028 run native platform acceptance; P045 reruns the
  exact cross-platform clean-machine matrix.
- The contract's INSTALL-JOURNEY-1 through INSTALL-JOURNEY-5 qualify primary
  Windows, DEB, RPM, AppImage and alternate quick-install surfaces.
- INSTALL-JOURNEY-6/7/8 qualify repair, data-preserving ordinary uninstall and
  interruption recovery for every advertised applicable format.
- FIRST-RUN-1 qualifies Connect through the first working library;
  FIRST-RUN-2 qualifies managed hosting, administrator setup and first library.
- P027 and P043–047 preserve profiles, credentials, databases, library/server
  content, bindings, checkpoint/intent/conflict identity and pause choices.
  Dangerous migrations need verified coordinated backup/restore and explicit
  rollback limits under ADR-021 and Prompt115.
- Native machines/VMs establish install, privilege, supervisor and launch
  evidence. Fixtures, cross-builds, static manifests and offscreen checks keep
  their narrower labels. Physical/VM power-loss evidence is separate from
  process-termination tests. Ignored/skipped tests are not passes.
- P046/P047 usability review checks simple copy, accessibility, progressive
  disclosure, safe recovery and the approximately five-minute target under
  stated conditions. Timing is a target, not a promised benchmark.
- P048 alone freezes the final `v0.2.0` source/artifacts, creates the final tag
  and may emit **SYNVEIL_V0_2_0_RELEASED** after remote verification.

See the exact paths/assertions in the
[acceptance journeys](INSTALLATION_PRODUCT_CONTRACT.md#13-exact-acceptance-journeys-for-later-automation).
An earlier checkpoint or a set of documents/builders does not complete later
prompts. Ordinary installation must succeed through UI alone; Advanced and
terminal alternatives cannot substitute for a failed primary journey.

## Scope and baseline discipline

Prompt001 is documentation only: no runtime behavior, dependencies, migrations,
installer code, host provisioning, version bump, tag or published assets.
Version remains `0.1.0`; server migrations `36`, client migrations `7`,
`LOCAL_SCHEMA_VERSION = 7`. Each later prompt explicitly determines whether
schema/version changes are authorized rather than inheriting authority here.

The v0.2 work stays within the named prompts. Do not add macOS/mobile or Synveil
OS implementation scope to this roadmap. The installation work does not expand
into unrelated synchronization semantics, authentication protocols, AI,
analytics, telemetry, mandatory relays or an unrequested automatic updater.
Existing native lifecycle, SecretStore, credential, migration and canonical
controller ownership must be preserved. Resume interrupted engineering work by
inspecting durable files and the worktree; never reset or clean unrelated work.

## Progress record

| Record | State |
| --- | --- |
| Prompt001 | Contract, architecture record and authoritative prompt map are documented; no installer or runtime behavior is implemented. |
| Prompt002 | Existing v0.1 installation/deployment surface audited; see the evidence manifest. No installer/runtime behavior was implemented by this audit. |
| Prompt003 | Installation architecture and component ownership documented; implementation and clean-machine delivery are not claimed. |
| Prompt004–048 | Planned by the exact named allocation above; no completion or clean-machine delivery is claimed. |

Future completion records must point to concrete source and validation evidence
and state unavailable gates honestly. A successful Prompt001 push is not a v0.2
product release. Only P048 owns the final v0.2.0 release/tag gate.
