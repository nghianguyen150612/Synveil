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
- CI repair resumed from branch head `ad8389aa587c9d5b88edc83f15702d1be8791ed0`.
- Source-integration repair head: `202f1f8339a49cc9b37c6e94119ecbf3919e64a0`.
- Focused repair commits: `2a803e5` (Linux-only Host IPC and build paths),
  `3517adb` (Host package inventory/ownership), `2120746` (SQLx TLS trust
  roots), `bad6038` (desktop/bootstrap contracts), `9cd6ac3` (AppImage input
  requirement), `0af1a83` (strict Host Clippy), `c34cb08` (binary-safe secret
  scanning), `b40c5dd` (encoded Rust flags in the remap probe), `e604c60`
  (remaining strict lint and Windows packaging contracts), `cf77160`
  (current-head CI, package-fixture and Windows workflow regressions), and
  `202f1f8` (exact Linux Host package ownership assertion).
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

## CI source-integration repair

The first automatic PR checks at `ad8389aa587c9d5b88edc83f15702d1be8791ed0`
exposed genuine source and contract defects. The repair kept Linux privileged
Host IPC behind Linux target gates and supplied an explicit unsupported Host
adapter for other desktop targets. Linux `nix` and privileged functionality
remain Linux-only; Windows/macOS Host support was not added.

Production storage code no longer embeds `CARGO_MANIFEST_DIR`. Development
checkout exclusions are derived at runtime from Cargo's conventional `target`
directory, while the reproducible Host build uses the shared Rust path-remap
policy. The release helper was rebuilt locally with that policy and passed the
unchanged private-path scanner. One embedded sysusers comment contained the
literal `/home/*` example and triggered the strict scanner; its wording now
describes an interactive home without embedding that path.

The automatic clean-machine/package builds then found that the secret scanner
joined NUL-delimited binary format fragments into a false PostgreSQL credential
match. The scanner now confines credential matching to printable URI bytes and
excludes braces used by public Rust format templates. It still rejects actual
credential URLs and both device-token prefixes. Regression fixtures cover
real credentials, tokens, NUL-separated fragments and format templates; the
release helper passes both artifact scans. The nested target-root probe now
decodes `CARGO_ENCODED_RUSTFLAGS`, the transport the reproducibility helper
actually exports, instead of reading the intentionally unset `RUSTFLAGS`.

The Rust CI runs at `e604c603f5d74a32ca955a13093d1e5c1491ec77` found three
strict Clippy findings in client sync: an oversized HTTP profile enum variant
and two collapsible conditional blocks. The profile is now boxed at the enum
boundary and the conditions were collapsed without changing behavior. These
warnings also stopped the Linux Qt script and the Connection Setup, Library
Wizard, Installation Progress, and Repair/Recovery quality jobs. The affected
client, sync and installer-engine Clippy command passes locally after the fix.

The same Rust run passed Linux, Windows and macOS cross-target workspace
checks, macOS tests, dependency policy and web quality. Ubuntu tests then found
that the adversarial Linux install fixture omitted the now-required Host
helper argument. Its package assertions also treated the Host helper as a
source file and excluded its reviewed destinations. The fixture now supplies
the disposable helper bytes, the manifest header names `BINARY_HOST_HELPER`,
and the closed-surface test admits only four exact Host helper/registrar/polkit
paths. All 23 local Linux adversarial cases pass. Windows tests failed while
parsing a CRLF checkout of the packager and passing a Windows path to Bash;
the packager is pinned to LF, the parser normalizes line endings, and the
syntax test feeds the script to Bash over stdin. All three local Windows
packaging contract tests pass.

On the first run at repaired head `c845ca3c3aa2f19ffcaeccacb77e6cd17e705039`,
Rust CI found another stale closed-surface assertion: the production packaging
test expected 13 package entries, while the approved manifest has 18 after
the five Host helper, registrar, policy and inspector entries. The test now
requires 18 entries and checks the source, mode, root owner/group and
PACKAGE-owned class for each exact Host path. All 11 production packaging
unit tests pass locally; the 124-test package integration contract also passes.

The Windows installer CI built its runtime payload, then stopped at the first
negative-closure fixture. The PowerShell job suppressed child diagnostics and
relied on `$LASTEXITCODE`; it now uses a bounded child process, captures both
streams, and requires a nonzero exit with the expected payload-identity error.
The Windows native-acceptance candidate producer succeeded, but its consumer
looked under `candidate/target/windows-installer/` after `upload-artifact`
flattened the common root. Consumer paths now match the flattened artifact,
and the source validator asserts this layout.

Linux package, clean-machine artifact-preparation, and PostgreSQL maintenance
jobs all stopped at ShellCheck on generated Debian `preinst`: an empty approved
upgrade list becomes `for source in ''; do`, which is SC2041. The suppression
now targets SC2041 with that exact explanation; the generated hook passes
ShellCheck 0.9.0 and shell syntax validation. The independent build paths were
not run past this gate on that head.

The source integration run also recorded two external-input gates. AppImage
failed closed before acquisition/build because no exact runtime file and
trusted SHA-256 were supplied. Server self-host acceptance stopped at artifact
identity because authentic signed server, PostgreSQL 17 and Caddy inventories
and publisher keyrings were absent; installed-host jobs did not run. The
Windows native-acceptance artifact authentication failure was the flattened
path mismatch above, so its interactive acceptance step did not run. Other
completed workflow slices for server services, first-admin bootstrap, Unified
Welcome, network reachability, installer security and installation resilience
passed on that source head. The parent-head Actions snapshot is recorded in
the updated inventory. Publication-time Actions for the repaired source head
are reported separately in the PR description and task closeout; cancelled or
skipped work is not a pass.

The closed Linux package surface now includes the Host helper, runtime prepare
and registration scripts, polkit policy, and read-only Host inspector service.
Manifest modes/owners, DEB/RPM dependencies, and the package contract were
reconciled together. The only automatic activation exception is the sandboxed,
read-only Host inspector; client and maintenance services remain inactive.
The older package lifecycle permission inventory now checks all five added
paths explicitly.

The remaining fixes were narrow: SQLx now uses OS trust roots without changing
TLS verification; `NativeManager._run` preserves `mutate` and forces native
elevation for mutation; P030 and Unified Welcome validators follow the current
authenticated acquisition and Host routing behavior; and the P035 workflow no
longer runs the same stateful PostgreSQL bootstrap test twice. Its source list
includes the workflow file so that this correction is checked on the PR.
Formatting and strict Host Clippy defects were corrected without changing Host
authorization, reviewed-plan binding, nonce/generation fencing, canonical
owner behavior, or the IPC JSON representation.

AppImage building remains fail-closed. The operator must provide the exact
type-2 runtime file and an independently trusted 64-character lowercase SHA-256
using `--runtime-file`/`--runtime-sha256` or the corresponding
`SYNVEIL_APPIMAGE_RUNTIME_FILE`/`SYNVEIL_APPIMAGE_RUNTIME_SHA256` variables.
The current build workflow has no authenticated runtime input, so it stops
before acquisition/build; the missing input is recorded as unavailable rather
than as a build result.

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

Local diagnostics and focused suites that passed:

- `cargo fmt --all -- --check`, `git diff --check`, `bash -n` on changed shell
  entry points, and Python byte-compilation of changed scripts.
- `cargo check -p synveil-server-host --all-targets --locked` and
  `cargo clippy -p synveil-server-host --all-targets --locked --no-deps -- -D warnings`.
- `cargo clippy -p synveil-server-service --all-targets --locked --no-deps -- -D warnings`
  and `cargo test -p synveil-server-service --locked`: pass, 4 service tests.
- `cargo test -p synveil-metadata --test release_artifact_units --locked`:
  8 passed, including artifact secret scanning and reproducible path remapping.
- `cargo test -p synveil-metadata --test windows_desktop_packaging_units --locked`:
  3 passed, checking the current bounded CRT and deterministic ZIP policy.
- `cargo test -p synveil-metadata --test linux_deployment_adversarial_units --test windows_desktop_packaging_units --locked`:
  23 Linux adversarial cases and 3 Windows packaging contract cases passed.
- `cargo test -p synveil-metadata --test production_packaging_units --locked`:
  11 passed, including the five exact Host package paths and ownership modes.
- `cargo clippy -p synveil-client -p synveil-client-sync -p synveil-install-engine --all-targets --locked --no-deps -- -D warnings`:
  passed after correcting the current-head client-sync findings.
- Windows native-acceptance source validation, Python byte-compilation,
  inventory JSON parsing, `bash -n`/`sh -n` and `git check-attr` for the pinned
  Windows packager line endings passed.
- `cargo test -p synveil-install-engine --locked` passed, including 124 package
  integration tests. The metadata Linux package and lifecycle suites passed 27
  and 37 tests; systemd unit fragments and service identity suites passed 3
  and 13 tests.
- Server/storage suites passed: server-storage 23 tests; local storage and
  object-store suites passed. Six live PostgreSQL storage tests remained
  ignored because no live database was configured.
- Python installer-security (23), quick-installer (15), Linux platform policy
  (11), and release manifest/download/channel suites passed. Documentation
  validation passed all 23 validators; package-integration validation passed
  its static checks and 124 Rust tests. Debian/RPM UX validators and exact
  ShellCheck validation passed.
- The local release Host helper passed both the private-path and secret-marker
  scanners. `cargo-deny check` passed all configured policy categories.

The full local workspace check could not find `dbus-1.pc`. The isolated Qt
toolchain did let `scripts/test-desktop-ui.sh` run: 92 desktop presentation
tests passed, then local linking failed because its static `libdbus-1.a`
references unavailable `sd_listen_fds` and `sd_is_socket` symbols. The full
desktop Clippy command was canceled after it began recompiling generated Qt
C++ inputs; the affected Rust-package Clippy command above completed. No
Windows cross linker or macOS SDK is present for local cross-target builds;
GitHub Actions supplied cross-target evidence on the pre-repair head. These
local constraints do not describe the corresponding hosted jobs. No Host
effect was run on this unsupported OS.

Automatic Actions are evidence only for jobs that completed. Earlier runs at
`0af1a83` and `c34cb08` exposed and recorded the artifact-gate and test-harness
defects described above; cancelled or superseded jobs are not counted as
passes. The final branch-head snapshot is recorded after the documentation
commit and includes pending, failed or cancelled runs.

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

The following acceptance and release qualification remains
**NOT_RUN_QA_DEFERRED** for this candidate. Source-level Actions results are
recorded separately and do not count as installed-Host qualification:

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

New/imported product behavior still needs native acceptance. Windows
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
