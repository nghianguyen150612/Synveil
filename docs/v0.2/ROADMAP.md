# Synveil v0.2 roadmap — Effortless Setup

Status: **Prompt001 product roadmap; implementation targets, not shipped claims**.
Released baseline: `v0.1.0` at
`fa23232ff0154f627ebdd221ec5435134f177af0`.

The objective is a clean supported computer → download → install → launch →
guided setup → first library → synchronization, without a terminal on the
ordinary desktop path. The [installation product contract](INSTALLATION_PRODUCT_CONTRACT.md)
and [ADR-049](../adr/ADR-049-v0.2-effortless-installation-architecture.md) govern
the work. The [v0.1 roadmap](../en/ROADMAP.md) remains historical context;
this document does not reclassify its future platform/features as v0.2 delivery.

The user supplied the seven phases below and explicitly selected a
scope-oriented allocation when no separate Prompt001–048 breakdown was supplied.
The ranges record that allocation as planning guidance, not a recovered list
of previously approved implementation prompts. Detailed later prompts must
define their own prerequisites, owners, allowed paths, tests and completion
gates. Scope adjustments must retain the release contract and be recorded.

## Phase allocation: Prompt001–048

| Phase | Prompts | Scope | Exit evidence |
| --- | --- | --- | --- |
| Installation contract | **001** | Product language, primary artifacts, installer/first-run boundary, ownership, recovery, acceptance journeys and this roadmap | Documentation validated; version/schema unchanged; one documentation commit pushed. No installer implemented. |
| Distribution foundation | **002–008** | Freeze OS/architecture/runtime support, release trust/signatures and verified acquisition, payload/component identities, common typed plans/results, ownership verification and platform adapter boundaries | Exact support matrix and trust bootstrap defined; safe plans and verification reject incompatible/tampered inputs; existing manifest/native manager remain authoritative. |
| Linux installation | **009–016** | Qualify DEB and RPM GUI install/update/removal, dependency closure and user integration; deliver AppImage and verified quick-install script; native recovery/repair | INSTALL-JOURNEY-2/3/4/5 plus Linux repair/uninstall/interruption variants pass on each named native environment. |
| Windows installer | **017–024** | Select and implement `SynveilSetup.exe`, per-user install, runtime closure, minimal screens/options, current-user integration, verified Finish/Open, repair/update/removal | INSTALL-JOURNEY-1 plus Windows repair/uninstall/interruption variants pass on qualified clean Windows. ZIP remains an optional advanced artifact. |
| Guided self-hosting | **025–034** | Close managed-database and host-supervisor decisions; define host component delivery, protected bootstrap, storage/access choices, readiness, compatible lifecycle and recoverable setup | FIRST-RUN-2's managed host foundation works without manual infrastructure; database/service/access decisions have the required evidence and accepted ADRs. |
| First-run UX | **035–042** | Deliver Host/Connect entry, canonical authentication/profile/library flows, safe folder selection, understandable progress/errors, accessibility, bilingual copy and resume behavior | FIRST-RUN-1/2 pass through application UI, retaining existing authentication/sync/root/unknown-outcome ownership. |
| Release hardening | **043–048** | Full clean-machine journey matrix, v0.1.0/supported-source upgrade preservation, backup/restore, repair/uninstall/power-loss drills, usability and release documentation/artifact verification | Native evidence for advertised platforms and all applicable journeys; frozen release policy satisfied; no engineering-doc dependency on ordinary installation. |

The allocation covers each prompt from 001 through 048 once. It is deliberately
scope-oriented: it does not pre-authorize features, prescribe every prompt's
implementation, or imply a version bump/tag/release date. Linux and Windows
work depend on distribution foundation; guided hosting needs approved component
acquisition and lifecycle plans; first-run integrates those capabilities;
release hardening cannot promote incomplete phases.

## Architectural decisions and their deadlines

| Decision | Owner | Required by / constraint |
| --- | --- | --- |
| Exact supported OS versions, Linux compatibility and GUI execution baseline | Platform / Distribution, Release | Distribution foundation, before package implementation/support claims. x86_64 only in the initial matrix. |
| Release signing, bootstrap-script trust and key rotation | Release, Security | Distribution foundation, before executing downloaded components. Checksums alone do not authenticate a release. |
| Windows installer technology and update/repair strategy | Distribution, Desktop, Security | Windows phase; must satisfy per-user/no-admin ordinary path and native lifecycle evidence. No framework is selected by Prompt001. |
| Managed PostgreSQL distribution (`OD-PLAT-001`, proposed ADR-019) | Database, Release, Security, Product | Before guided-host provisioning code. Preserve canonical PostgreSQL; no SQLite server alternative. |
| Host supervisor/elevation (`OD-PLAT-002`) | Platform, Release, Security | Before host service implementation. Existing least-privilege desktop client supervision stays under ADR-041. |
| Offered access modes (`OD-PLAT-003`, proposed ADR-020) | Networking, Security, Product | Before new remote-access mechanisms. Local/private bootstrap, explicit reach, no silent public exposure or mandatory proprietary relay. |
| Supported upgrade sources and rollback/restore limits | Database, Client, Release | Before release hardening; include v0.1.0 fixtures, unknown/future-schema rejection and coordinated restore evidence. |
| Exact diagnostic limits and product copy | Desktop, Product, Security, Accessibility | Before first-run/error surfaces ship; finite typed categories and bounded redaction. |

The open platform decisions remain in [PLATFORM.md](../en/PLATFORM.md).
Prompt001 fixes the installation-versus-first-run product boundary and primary
artifact policy. It does not silently accept proposed database/remote-access
ADRs or select a new privileged host supervisor.

## Required evidence and promotion rules

- INSTALL-JOURNEY-1 through INSTALL-JOURNEY-5 qualify the primary Windows,
  DEB, RPM, AppImage and alternate quick-install surfaces.
- INSTALL-JOURNEY-6/7/8 qualify repair, data-preserving ordinary uninstall and
  interruption recovery for every advertised applicable format.
- FIRST-RUN-1 qualifies Connect through the first working library;
  FIRST-RUN-2 qualifies managed hosting, administrator setup and first library.
- Supported upgrades preserve profiles, credentials, databases, library/server
  content, bindings, checkpoint/intent/conflict identity and pause choices.
  Dangerous migrations need verified coordinated backup/restore and explicit
  rollback limits under ADR-021 and Prompt115.
- Native machines/VMs establish install, privilege, supervisor and launch
  evidence. Fixtures, cross-builds, static manifests and offscreen checks must
  retain their narrower labels. Physical/VM power-loss evidence is separate
  from process-termination tests. Ignored/skipped tests are not passes.
- Usability review checks simple copy, accessibility, progressive disclosure,
  safe recovery and the approximately five-minute target under stated
  conditions. Timing is a target, not a promised benchmark.

See the exact paths/assertions in the
[acceptance journeys](INSTALLATION_PRODUCT_CONTRACT.md#13-exact-acceptance-journeys-for-later-automation).
No phase is complete merely because its documents or builders exist. Ordinary
installation must succeed through UI alone; Advanced and terminal alternatives
cannot substitute for failed primary journeys.

## Scope and baseline discipline

Prompt001 is documentation only: no runtime behavior, dependencies, migrations,
installer code, host provisioning, version bump, tag or published assets.
Version remains `0.1.0`; server migrations `36`, client migrations `7`,
`LOCAL_SCHEMA_VERSION = 7`. Each later prompt explicitly determines whether
schema/version changes are authorized rather than inheriting authority here.

v0.2 installation work does not expand into new synchronization semantics,
authentication protocols, mobile/macOS support, AI, analytics, telemetry,
mandatory relays or an unrequested automatic updater. Existing native lifecycle,
SecretStore, credential, migration and canonical controller ownership must be
preserved. Resume interrupted engineering work by inspecting durable files and
the worktree; never reset/clean unrelated changes.

## Progress record

| Record | State |
| --- | --- |
| Prompt001 | Contract and architecture specification in this documentation change; checks and exact manifest are recorded in [PROMPT001_MANIFEST.md](PROMPT001_MANIFEST.md). |
| Prompt002–048 | Planned scope only; no completion or clean-machine delivery claimed. |

Future completion records must point to concrete source/validation evidence
and state unavailable gates honestly. A documentation target is not a released
capability, and a successful Prompt001 push is not a v0.2 product release.
