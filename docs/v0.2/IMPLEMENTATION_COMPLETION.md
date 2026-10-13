# Synveil v0.2 implementation completion record

Date: 2026-10-13 (Asia/Bangkok). Mode: **DEVELOPMENT_FIRST_QA_DEFERRED**.

Current state: **IMPLEMENTATION_COMPLETE_QA_PENDING**.

The G01-G08 production source paths are implemented and connected. Authentic
PostgreSQL/Caddy/server runtime inputs remain an external deployment input
blocker; the verified supplied-input path is complete and fails closed until
those inputs are registered. Native acceptance and release qualification are
still pending.
No official stable tag, release, signing, distribution action or P048 action
is authorized or claimed by this record.

## Source and integration identity

- Repository: <https://github.com/nghianguyen150612/Synveil>.
- Actual fetched starting `origin/main`: `3851ea11927e24255614cfc38adbaccbd345ca03`.
- Development branch: `feat/v02-implementation-closeout`, based on that main.
- Product source checkpoint before documentation: `c56ea4c5f9b5b4e2643e6815711a415b70883d88`.
  Later documentation/publication commits do not change this product checkpoint.
  The PR and final task report identify the published branch HEAD.
- P047 PR [#104](https://github.com/nghianguyen150612/Synveil/pull/104):
  `release/p047-final-rc-validation`, head
  `81303a469beb2edd5ae7c3f5efacfc617b405093`, OPEN / DRAFT / UNMERGED when inspected.
- P045 PR [#78](https://github.com/nghianguyen150612/Synveil/pull/78):
  `feat/cross-platform-clean-machine-matrix`, head
  `035497c05c145e5c6129e744f9455937069680f4`, OPEN / DRAFT / UNMERGED when inspected.
- Existing `work`, upstream branches, commits and worktrees were preserved.
  Local `feat/v02-implementation-closeout-source-history` retains the intermediate
  integration history; the published branch imports each original commit once.
- Workspace/product version remains `0.1.0`. A version string is not evidence
  of a completed v0.2 product or a production release. No schema/migration was
  removed or rewritten for this closeout.

## Audit method and authoritative inventory

The authoritative allocation is [ROADMAP.md](ROADMAP.md), P001 through P048.
P042A is supporting historical checkpoint evidence, not a 49th feature prompt.
The [machine-readable inventory](IMPLEMENTATION_INVENTORY.json) contains every
prompt, its classification, a source anchor and the inspected behavior.

Inspection followed the QML/bridge/controller/local-IPC/client/sync call paths,
installer and native-package mutation paths, and server-owner composition.
The audit checked for real production callers, durable owners, effect application
and recovery boundaries. A plan struct or source filename alone was not counted
as an executable product journey. No new runtime evidence was obtained.

`IMPLEMENTED` below describes seven completed documentation/decision scopes.
`IMPLEMENTED BUT UNTESTED` describes 35 source implementations whose correctness
at this candidate still needs verification. The four previously partial Host
prompts are now implemented in source; six acceptance/release scopes are
`DEFERRED TO QA`. There are no newly asserted `MISSING` prompt bodies. G01-G08
are complete source implementations with native QA pending.

| Prompt | Authoritative scope | Classification | Source anchor |
| --- | --- | --- | --- |
| P001 | Effortless Installation Product Contract and Blueprint | IMPLEMENTED | [source](../../docs/v0.2/INSTALLATION_PRODUCT_CONTRACT.md) |
| P002 | Existing Installation Surface Audit | IMPLEMENTED | [source](../../docs/v0.2/EXISTING_INSTALLATION_SURFACE_AUDIT.md) |
| P003 | Installation Architecture and Component Ownership | IMPLEMENTED | [source](../../docs/v0.2/INSTALLATION_ARCHITECTURE.md) |
| P004 | Clean-Machine Acceptance Harness | DEFERRED TO QA | [source](../../docs/v0.2/CLEAN_MACHINE_ACCEPTANCE.md) |
| P005 | Unified Release Artifact Manifest | IMPLEMENTED BUT UNTESTED | [source](../../scripts/release_manifest.py) |
| P006 | Release Download and Integrity Contract | IMPLEMENTED BUT UNTESTED | [source](../../scripts/release_download.py) |
| P007 | Installer Common Engine | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/engine.rs) |
| P008 | Installation Transaction Journal | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/journal.rs) |
| P009 | Upgrade / Repair / Uninstall Common Contract | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/lifecycle.rs) |
| P010 | Installer Error Model | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/error_model.rs) |
| P011 | Release Channel and Version Selection | IMPLEMENTED BUT UNTESTED | [source](../../scripts/release_channel.py) |
| P012 | Linux Package Integration Reconciliation | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/linux_package.rs) |
| P013 | Debian/Ubuntu Desktop Package UX | IMPLEMENTED BUT UNTESTED | [source](../../deploy/packages/build.sh) |
| P014 | Fedora/RPM Desktop Package UX | IMPLEMENTED BUT UNTESTED | [source](../../deploy/packages/rpm/synveil.spec.tmpl) |
| P015 | AppImage Build Foundation | IMPLEMENTED BUT UNTESTED | [source](../../deploy/packages/build-appimage.sh) |
| P016 | AppImage Runtime Integration | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/appimage.rs) |
| P017 | Linux Quick Install Script | IMPLEMENTED BUT UNTESTED | [source](../../scripts/linux_quick_install.py) |
| P018 | Linux Distro and Architecture Detection | IMPLEMENTED BUT UNTESTED | [source](../../scripts/linux_platform_detection.py) |
| P019 | Linux First Launch Integration | IMPLEMENTED BUT UNTESTED | [source](../../crates/client/src/launch.rs) |
| P020 | Linux Clean-Machine Acceptance | DEFERRED TO QA | [source](../../docs/v0.2/LINUX_CLEAN_MACHINE_ACCEPTANCE.md) |
| P021 | Windows Installer Technology Decision | IMPLEMENTED | [source](../../docs/v0.2/WINDOWS_INSTALLER_TECHNOLOGY_DECISION.md) |
| P022 | SynveilSetup.exe Skeleton | IMPLEMENTED BUT UNTESTED | [source](../../deploy/windows/installer/Synveil.iss) |
| P023 | Windows Installer UI | IMPLEMENTED BUT UNTESTED | [source](../../deploy/windows/installer/Synveil.iss) |
| P024 | Windows Runtime Deployment | IMPLEMENTED BUT UNTESTED | [source](../../deploy/packages/build-windows.sh) |
| P025 | Windows Per-User Installation | IMPLEMENTED BUT UNTESTED | [source](../../deploy/windows/installer/Synveil.iss) |
| P026 | Windows Startup Integration | IMPLEMENTED BUT UNTESTED | [source](../../crates/client/src/launch.rs) |
| P027 | Windows Repair / Upgrade / Uninstall | IMPLEMENTED BUT UNTESTED | [source](../../deploy/windows/installer/Synveil.iss) |
| P028 | Windows Native Acceptance CI | DEFERRED TO QA | [source](../../docs/v0.2/WINDOWS_NATIVE_ACCEPTANCE.md) |
| P029 | Server Setup Product Contract | IMPLEMENTED | [source](../../docs/v0.2/SERVER_SETUP_PRODUCT_CONTRACT.md) |
| P030 | Server Dependency Strategy | IMPLEMENTED | [source](../../docs/v0.2/SERVER_DEPENDENCY_STRATEGY.md) |
| P031 | Managed Server Configuration | IMPLEMENTED BUT UNTESTED | [source](../../crates/server-config/src/store.rs) |
| P032 | Storage Location Wizard | IMPLEMENTED BUT UNTESTED | [source](../../crates/server-storage/src/wizard.rs) |
| P033 | Server Service Installation | IMPLEMENTED BUT UNTESTED | [source](../../crates/server-host/src/native/services.rs) |
| P034 | Server Network and Reachability Setup | IMPLEMENTED BUT UNTESTED | [source](../../crates/server-host/src/native/network.rs) |
| P035 | Server First-Admin Bootstrap | IMPLEMENTED BUT UNTESTED | [source](../../crates/auth/src/service.rs) |
| P036 | End-to-End Self-Host Wizard | IMPLEMENTED BUT UNTESTED | [source](../../crates/server-host/src/native.rs) |
| P037 | Unified Welcome Experience | IMPLEMENTED BUT UNTESTED | [source](../../crates/desktop/src/host_ui.rs) |
| P038 | Connection Setup Simplification | IMPLEMENTED BUT UNTESTED | [source](../../crates/client/src/controller.rs) |
| P039 | Authentication UX Polish | IMPLEMENTED BUT UNTESTED | [source](../../crates/client-sync/src/host.rs) |
| P040 | Library First-Run Wizard | IMPLEMENTED BUT UNTESTED | [source](../../crates/client-sync/src/host.rs) |
| P041 | Installation-to-Sync Progress Experience | IMPLEMENTED BUT UNTESTED | [source](../../crates/desktop/src/presentation.rs) |
| P042 | Repair and Recovery UX | IMPLEMENTED BUT UNTESTED | [source](../../crates/client/src/launch.rs) |
| P043 | Installer Security Hardening | IMPLEMENTED BUT UNTESTED | [source](../../scripts/release_download.py) |
| P044 | Installation Resilience Hardening | IMPLEMENTED BUT UNTESTED | [source](../../crates/install-engine/src/journal.rs) |
| P045 | Cross-Platform Clean-Machine Matrix | DEFERRED TO QA | [source](../../docs/v0.2/ROADMAP.md) |
| P046 | v0.2 Documentation and Distribution Readiness | IMPLEMENTED | [source](../../docs/en/v0.2/README.md) |
| P047 | v0.2 Final Release Candidate Validation | DEFERRED TO QA | [source](../../docs/v0.2/ROADMAP.md) |
| P048 | Synveil v0.2.0 Release Gate | DEFERRED TO QA | [source](../../docs/v0.2/ROADMAP.md) |

Every unexecuted test or journey in this inventory is
**NOT_RUN_QA_DEFERRED**. Historical evidence belongs to its recorded source,
platform and artifact, not automatically to this development candidate.

## Implemented product paths

| Surface | Source assessment and limits |
| --- | --- |
| Desktop | Native Qt/QML entry point, tray, Welcome, Connect, server-address validation, profile persistence, one-time device enrollment, first-library creation/native folder selection, bounded progress and recovery actions have client/controller/IPC owners. The supported-platform Host wizard runs preflight, review/consent, owner creation/login, first Library, Ready verification and Connect handoff through `NativeHostOwners`. |
| Client/sync | Profile/device-bound verified HTTPS, library creation/discovery for setup reconciliation, durable bindings/checkpoints/intents, initial inbound/outbound cycles, bounded upload/download, interrupted-operation recovery, safe filesystem paths, explicit conflicts, local credential cleanup and supervised shutdown/restart are wired. Existing-server Connect requires an administrator-issued one-time enrollment grant. |
| Server | PostgreSQL-authoritative metadata, transactional initial administrator creation, API/library/sync routes, canonical migration entry point, worker and graceful shutdown exist. P031 configuration, P032 storage, PG17 provisioning, native systemd services, TLS/reachability, managed dependency catalog, and P036 canonical-owner composition have production source paths. Authentic dependency inputs and installed-host verification remain pending. |
| Windows installer | Lowest-privilege per-user Setup, terms/options, launch/startup choices, closed payload ownership, exact repair, compiled upgrade policy, obsolete-file reconciliation, registered preserving uninstall and reinstall exist in source. |
| Ubuntu/Fedora installers | Native manager ownership, launcher/Qt dependencies, preserving hooks, package verification and same-version repair exist. Reviewed older stable versions can now be compiled into pre-unpack policy; no older transition is currently approved. |
| AppImage | The builder preserves AppRun; current-user integration, repair, relocation, removal and explicit background-client options exist. Output now requires an exact runtime file and trusted digest. Native execution and a production runtime identity remain pending. |
| Packaging | DEB/RPM, Windows runtime ZIP/Setup and AppImage producers exist. Linux packages include the purpose-specific Host helper, inspector, polkit policy, and signed-input registrar. The Host bundle producer/assembler/registrar path verifies supplied inputs; production runtime bytes and publisher keyrings remain external inputs. |
| Documentation | English/Vietnamese preview guides retain unsupported cases and Host limitations. This inventory, provenance and gap report distinguish implementation from qualification. |

## Integration completed

The [source-import ledger](IMPLEMENTATION_SOURCE_IMPORTS.json) maps all 68
selected P047 commits and two P045 product commits to their development commit,
original author and exact imported paths. Git mail patches retain authorship;
`Upstream-commit` and `Upstream-PR` trailers retain traceability. The ledger
accounts for every changed P047 path, including deliberately omitted paths.
No complete P045 or P047 branch was merged.

Relevant P047 fixes include:

- Windows named-pipe cancellation ownership and packaged-client resolution
  during Task Scheduler cleanup.
- Windows installer journal directory/file durability and recreated configured
  storage-root rejection; canonical storage exclusions remain enforced.
- Atomic-save renames over an existing synchronized file now produce content
  modification intent for the existing node.
- AppRun preservation, app-local CRT closure, audited inbox PE imports,
  deterministic ZIP/DEB/Qt/native compiler output and shared tool selection.
- Native package installation/verification authorization and trusted dpkg
  documentation-exclusion handling; RPM scriptlets remain printable.
- Restore mutation recovery refresh after browser session restoration.
- Cross-platform source/test compilation fixes and directly associated
  regression coverage. That coverage was retained, not executed locally.

P045 product-only imports are `08b4dcb9a0ba9f767ee574b231b203f068b1fb11`
(existing-profile-only background startup) and
`15c00cfdce2bdd495020457826e4beed1fb3ecbe` (failed migration connection ownership).
The successor named-pipe instance strategy was reconciled with the P047 fix.
P045 VM provisioning, forwarding, image locks, interactive Windows/logon
harnesses, matrix workflows and native evidence reports remain separate.

The only P047 workflow behavior adapted here is existing Windows build-job
input/tool selection. No acceptance job, protected-branch rule, gate or workflow
trigger was removed, disabled or newly dispatched. Source assertions follow
the actual selected tool and closed CRT policy; their native acceptance
requirements remain in place and no new PASS is claimed.

## Newly implemented or corrected source

- Host reviewed-plan binding covers individually length-framed artifacts and
  the ordered privileged effect list. Consent is consumed before mutation;
  cancellation clears intent and a later attempt must inspect/review again.
  Ready requires supported platform and complete handoff fields.
- Managed-edge rendering now binds the reviewed listener, uses the protected
  systemd leaf-key path and public certificate, disables automatic HTTP
  redirects/admin control, blocks remote first-admin mutation, and replaces
  forwarding metadata from direct-peer/request evidence. Origin input is
  bounded and normalized; modes/listeners/firewall plans must agree.
- The server runtime producer resolves output safely, stages privately,
  derives its version from Cargo, inventories actual bytes plus notices, and
  publishes without replacing an existing runtime identity. Reruns may reuse
  only an identical closed inventory; it never deletes an old published bundle.
- AppImage output requires `--runtime-file` and `--runtime-sha256` (or the
  corresponding `SYNVEIL_APPIMAGE_RUNTIME_*` variables). The staged runtime is
  verified before passing `LDAI_RUNTIME_FILE` to the output plugin. No runtime
  digest or production artifact was invented in this task.
- The MSVC selector exports the same authenticated tools for local PowerShell
  and existing Actions build jobs; local builds do not need `GITHUB_ENV`.
- [native_upgrade_policy.py](../../scripts/native_upgrade_policy.py) compiles
  a bounded, duplicate-field-rejecting, version-bound approved-source inventory
  into DEB/RPM pre-unpack hooks. The checked-in
  [native policy](../../deploy/release/native-upgrade-policy.json) approves no
  older sources. Future entries require a reviewed older product version and
  its exact RPM release; policy population is separate from implementing the
  hook. Quick install remains a fresh/same-version alternate, not an updater.
- Added the production Linux `CanonicalOwners` adapter and a narrow pkexec
  helper session. The helper binds bounded typed IPC to caller UID, generation,
  reviewed effects and one-time nonces; its inspector accepts read-only status.
- Added the owner journal, managed PostgreSQL 17 provisioning, dedicated
  systemd identities/units, API/worker health checks, certificate and exact
  firewall ownership, lifecycle review, and durable network-reconfiguration
  intent that is cleared only after verified reachability.
- Replaced the Host availability page with the Qt/QML guided journey and wired
  profile-specific public CA trust to verified TLS and enrollment. Origin
  changes fence old credentials and preserve the trust descriptor across an
  interrupted handoff.
- Added a fail-closed signed-input workflow for PostgreSQL, Caddy, server and
  helper payload registration. Authentic publisher inputs are not fabricated.
- Failed migration teardown closes the pool even if connection close fails;
  Windows pipe successor allocation precedes promotion of a connected client.

## G01-G08 implementation status

| ID | Status and production path | Owner, security boundary, persistence and recovery |
| --- | --- | --- |
| G01 | COMPLETE_SOURCE — `DesktopUiBridge` → `HostSession` → `NativeHostOwners` → `CanonicalOwners`; `/usr/libexec/synveil-host-helper`. | `pkexec` is explicit. The root helper accepts bounded typed requests, validates the calling UID, full plan/effect order/generation/nonce and a root lock. Native commands are fixed and allowlisted. Durable intents and owner inspection govern retry. |
| G02 | COMPLETE_SOURCE / DEPENDENCY_INPUT_UNAVAILABLE — `prepare-host-runtime.py`, `assemble-host-inputs.py`, `register-host-inputs.py`, `build-host-server-input.sh`; root catalog/cache consumed by `NativeOwners::catalog`. | Detached signatures, independent keyrings, exact OS/architecture, version, provenance, licenses, closed inventory and SHA-256 are checked before digest-addressed atomic publication. No runtime executes from the supplied bundle. Real signed server/PostgreSQL/Caddy inputs and publisher keyrings are not present. |
| G03 | COMPLETE_SOURCE — `services::prepare_postgres`, `review_postgres_repair`, and `provision_database`; existing P031 config and P033 migration owners. | The helper initializes only an owned-empty PG17 directory, binds its device/inode and system ID, persists a private port, validates the live local SQL listener against the same cluster identity, and reconciles protected roles/database without resetting data. A separately reviewed repair archives an owned partial `initdb` directory intact, durably records its inode, then resumes in a new empty directory. |
| G04 | COMPLETE_SOURCE — `services::install_runtimes`, `reconcile`, `stop_owned`, `remove_owned`, `effective_unit`; compiled systemd templates. | Dedicated identities and exact unit/drop-in/runtime inventories are checked. Start, stop, restart, migration, health, upgrade retention and software removal use bounded systemd calls. Removal preserves database, storage, credentials and CA. |
| G05 | COMPLETE_SOURCE — `network::certificates`, `configure`, `firewall_apply`, `remove_owned_firewall`; `ReviewNetwork` and `ReconfigureNetwork` purpose. | P031 stores protected keys; X.509 constraints, key match, SAN, chain and expiry are verified. Caddy uses the leaf credential only. Firewall changes target one reviewed interface/rule and are queried before retry/removal. Changed access requires a fresh reviewed plan. |
| G06 | COMPLETE_SOURCE — QML Host journey, `DesktopUiBridge`, `HostSession`, first-admin/account and desktop handoff. | Preflight, profile/storage/access selection, review, consent, progress, owner creation/sign-in, first Library, Ready and Connect handoff call canonical owners. Connect remains on its existing controller path; secret fields clear after submit. |
| G07 | COMPLETE_SOURCE — `HostTrust`, profile-scoped trust sidecar, strict HTTP/TLS transport, `configure_and_approve_managed_host`. | The descriptor binds server, network, origin, CA and generation to one profile. Normal chain/hostname/expiry checks remain active. Origin changes fence old credentials and preserve the previous descriptor across interruption until explicit verified handoff. A different private CA is rejected for preserving operator recovery; no silent replacement is accepted. |
| G08 | COMPLETE_SOURCE — root journal, `OwnerObservation`, read-only inspector and the 15-second recovery UI poll. | Resume state comes from actual P031-P035 owners, file/runtime intents, PG marker and live systemd/HTTP/DB/worker evidence. Network changes persist a pending selection before effects and clear it only after verified reachability. Partial PostgreSQL repair persists a pending archive intent before atomic rename, verifies either side after restart, and retains every archived directory. Ready is never a stored boolean. |

All G01-G08 source paths target Ubuntu 24.04 and Fedora 42 x86_64. No source
gap remains in the declared v0.2 Host flow. Missing authentic runtime bytes,
signatures and trusted publisher keyrings remain deployment inputs; no expected
identity is fabricated or checked in.

## Checks and evidence preserved

The recovered source received selective diagnostics only: Rust formatting,
`cargo check -p synveil-desktop -p synveil-api -p synveil-client-sync`, Python
byte-compilation for the three Host input tools, Bash/POSIX shell syntax checks,
and `git diff --check`. These checks do not run services or qualify runtime
artifacts. The targeted Cargo check includes Host helper and Qt/CXX source
generation/type checking; it is not a linked desktop build or installed Host.

Automatic source checks on the first published documentation head identified
two Rust formatting differences. Both were corrected. A focused rustfmt check
then passed for `server-bootstrap/src/lib.rs`, `server-network/src/lib.rs`,
`client/src/control.rs` and `client-sync/src/local_migrations.rs` using an
isolated formatter and matching driver libraries verified against Rust's
published stable tool manifest. No compiler or product test was invoked.

Rust and Qt build inputs used for the targeted source check were assembled in an
isolated toolchain under `/workspace/toolchains`; no system packages were
installed. No Host effect was run because the current Debian 13 environment is
outside the supported Host platforms. No workspace/web/installer suite, P036,
P045, P047, native GUI, service/reboot, or power-cycle test was executed. All
unexecuted suites remain **NOT_RUN_QA_DEFERRED**.

P047 report and historical source evidence remain unchanged on PR #104.
Previously recorded qualification is **21 gates: 1 PASS, 20 BLOCKED**;
scoped actual results are **14 PASS, 7 BLOCKED**. Those counts were not changed
and do not qualify this development source. No release report was imported to
suggest otherwise. P045 historical evidence also remains on its original PR.

Existing GitHub Actions may run automatically on push/PR events. The final
task report records the actual publication-time snapshot, including failures,
queued/in-progress runs or unavailable status. Automatic workflow results do
not silently promote the implementation or release state.

## Deferred QA, environments and release preparation

All of the following are **NOT_RUN_QA_DEFERRED** for this candidate:

- Product Rust/web and focused regression suites, compiler/Qt integration,
  source quality and dependency-policy checks beyond the syntax diagnostics
  explicitly listed above.
- P036 installed Host: real production dependency inventories, storage/admin/
  library/first-sync journey, local HTTPS/LAN, service restart/reboot, repair,
  reinstall, response-loss recovery and preservation.
- P020/P028/P045 clean Windows 11 interactive/logon/IPC and Ubuntu 24.04/
  Fedora 42 x86_64 desktop/installer journeys; generic Linux AppImage runtime.
- P044 physical/VM power-cycle, interrupted upgrade/disk-full and coordinated
  data preservation/restore checks.
- The full P047 21-gate suite, raw artifact rehashing, reproducibility and
  production payload/dependency inventory verification at the eventual source.
- Production signing, trusted bootstrap/key provisioning/rotation,
  authenticated channel freshness, download/installed verification and
  distribution-channel qualification.

No VM, external native runner or signing infrastructure was provisioned.
No stable release artifact or `v0.2.0` tag was published. P048 remains deferred.

## Known risks and state transitions

New/imported product changes remain untested at this candidate. Windows
directory flushing/pipe/task ownership, installed native authorization,
Qt runtime behavior and Caddy rendering need their appropriate later
verification. Existing CI source assertions and fixtures are preserved and
updated for the helper/input registrar package inventory; gates may not be
weakened or reported as passed without execution.
The newly required AppImage runtime input is intentionally absent until real
trusted bytes and a digest are supplied, so an unconfigured build fails closed.

Windows and Linux approved upgrade inventories are currently empty; they do
not promise an approved v0.1-to-v0.2 transition. Release preparation must select
the intended version and genuinely compatible sources without bypassing
migration, downgrade, integrity or preserving-repair boundaries. The server
binary bundle is not a complete or trusted installed Host distribution.

Managed Host support targets Ubuntu 24.04/Fedora 42 x86_64 only. Windows/macOS/
ARM managed Host, mobile/iOS, Synveil OS, a mandatory relay, an automatic updater
and unrelated sync/authentication redesign are outside the authoritative v0.2
scope. Existing first-library setup creates a library; arbitrary remote-library
attachment is not silently introduced by this closeout.

Completion requires G01-G08 to be implemented and source integrations recorded
truthfully. Then the project may state **IMPLEMENTATION_COMPLETE_QA_PENDING**
without waiting for native acceptance. **RELEASE_QUALIFIED** additionally requires
real tests, exact artifact/dependency verification, signing/bootstrap and channel
qualification. No such release qualification is granted here.
