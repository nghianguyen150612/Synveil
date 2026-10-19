# Synveil v0.2 Release Hardening R02 Manifest

## Closeout status

**R02_SOURCE_COMPLETE_WITH_BLOCKERS.** The source implementation, local regression work, fail-closed artifact contract, and executable acceptance harnesses are complete. Managed Host production qualification is not complete: no authentic production runtime bundle, publisher trust inputs, authorized native runners, or disposable VM were available. No native Host journey, boot/reboot, or P036 acceptance pass is claimed. No release tag or GitHub Release was created.

Source validation was performed on **0bed7c034aca35d5db51df001f6067a49867742b**. It contains implementation commit **8a5d5c9** and the separately scoped PostgreSQL scheduler fixture repair **0bed7c0**. Follow-up commit **045dc1e17cf6e980bc83ceca7c77a830e967ea28** adds only Windows-safe test teardown and records the observed CI failure; it does not change production behavior. The current documentation closeout head is reported in the final handoff because a commit cannot embed its own hash.

## A. Starting state and integration base

- Repository: **nghianguyen150612/Synveil**.
- Verified origin/main at task start: **b01c3b2328b271529d02cb4ee30d97be43a42611**.
- R01 PR [#148](https://github.com/nghianguyen150612/Synveil/pull/148) was open and unmerged at **24f84d6e9460d82380f3ab017a3a2d36d24f2879**. No R01 merge SHA exists.
- R02 branch: **feat/v02-managed-host-production-readiness**.
- R02 PR [#149](https://github.com/nghianguyen150612/Synveil/pull/149) is stacked on **fix/v02-release-ci-stabilization**, based on the verified R01 head above. It remains open and must not be merged ahead of R01. No merge SHA exists.
- No squash, force-push, tag, or public release was used.

## B. Implementation and connected production paths

Inspection of the requested roadmap, completion, dependency, service installation, managed configuration, wizard, R01 manifest, tracker, P036 workflow, end-to-end validator, and the server/storage/network/configuration/authentication/bootstrap/API/worker call paths showed that Managed Host provisioning, durable configuration, storage identity, PostgreSQL initialization and repair, network ownership, migration, API/worker lifecycle, and data-preserving uninstall were already connected. R02 completed gaps in runtime trust, readiness/ordering, install reconciliation evidence, inventory enforcement, and runnable native acceptance. It did not replace those existing flows.

### PostgreSQL 17 provisioning

- Personal/Home Managed Host retains Synveil-managed PostgreSQL 17. Advanced/Server retains operator-owned external PostgreSQL and other services.
- Runtime registration consumes explicitly supplied, signed PostgreSQL and server/Caddy inputs. It verifies the detached signature against an independently supplied trusted keyring; exact version and PostgreSQL major; platform and architecture; upstream source revision and HTTPS source identity; Synveil packaging revision; license/notice bytes; and the closed payload inventory with independently recalculated SHA-256 values. Runtime version probes run from a private copy of the already verified executable. There is no ambient download, guessed upstream checksum, or unsigned fallback.
- The approved immutable runtime location remains **/opt/synveil/postgresql/17/<runtime-id>/**. Persistent cluster data remains **/var/lib/synveil/postgresql/17/data**.
- PostgreSQL runs as **synveil-postgres**. Its endpoint is a reviewed private socket/loopback endpoint, authentication remains SCRAM, and the configured port is preserved. Existing cluster identity and PostgreSQL system identity are reconciled before reuse. Provisioning does not re-run initdb for an existing cluster, reset passwords, drop/recreate roles or databases, replace a database with an empty one, or move durable data into a replaceable runtime directory.
- Existing-cluster reopening, idempotent credentials, readiness and interrupted-state reconciliation remain part of the existing privileged Host path. Normal software removal preserves PostgreSQL data and server objects.
- The systemd PostgreSQL unit now models the actual pg_ctl forking service: bounded start/readiness, PID file, stop/reload timeout, restart policy, dedicated identity, and sandboxing. The previous notify declaration did not match the launched process model.
- **Runtime acquisition result: BLOCKED_EXTERNAL_INPUT.** Authentic PostgreSQL 17 runtime bytes, signed manifest/signature, approved publisher keyring/fingerprint, and license/notice files were not supplied. No digest or runtime was fabricated.

### Caddy, HTTPS, and network boundaries

- Local-only mode and explicitly approved private-LAN mode remain distinct. The API listener remains local behind the managed edge; PostgreSQL remains private and is never opened to LAN as a side effect of enabling HTTPS/LAN access.
- The configured canonical origin, TLS trust, hostname and certificate validation, and managed edge ownership are retained. There is no certificate bypass, undocumented HTTP downgrade, or mandatory relay.
- Caddy input follows the same signed, exact-version, source/platform/architecture, license/notice and payload verification path. Edge startup waits for network-online only where the configured LAN mode needs it; network reconciliation and repair continue through the existing native network owner.
- Advanced/Server continues to use operator-managed external edge/database services. Personal/Home owns only its approved local managed dependencies.
- **HTTPS native validation: NOT_EXECUTED.** No authenticated Caddy artifact or supported native Host runner was available. Static source and network contract checks passed; they are not a certificate or service-start qualification.

### systemd and privilege boundaries

- Preserved topology: **synveil-server.target → synveil-postgresql.service → synveil-server-migrate.service → synveil-api.service and synveil-worker.service**, with the managed edge ordered according to local/LAN configuration.
- PostgreSQL uses **synveil-postgres**. Migration, API and worker use **synveil**; the edge has its dedicated restricted identity. Credential delivery uses systemd LoadCredential where configured, protected configuration and secret files, restrictive service sandboxing, bounded startup/restart behavior, and ordered shutdown. No recursive ownership rewrite or unexpected privileged service execution was introduced.
- API and worker health evidence is checked against current server, storage, database and process identity and fresh successful worker-cycle evidence; process existence alone is not readiness.
- Unit syntax was checked with **systemd-analyze verify** against all six units using the same parser-only fixture-executable approach as CI. This is **PASS_STATIC** and proves neither executable compatibility nor service start. The exact PR workflow **server-services #39187655134** passed its static-contract job, including the six-unit parser check.
- Native systemd start, health, shutdown, boot and reboot acceptance: **NOT_EXECUTED**.

### P036 production inventory and Host binding

- Added versioned strict inventory schema **acceptance/p036/production-artifacts.schema.json**. The required inventory is an eight-entry closed matrix for desktop, server, postgresql17 and edge on Ubuntu 24.04 x86_64 and Fedora 42 x86_64.
- Each entry is bound to an exact artifact path and file size, independently recalculated SHA-256, version, source revision, platform, architecture, provenance, license and notice. Detached signatures are verified against the independently provisioned trusted keyring and exact pinned signer fingerprints. Runtime/package version probes and source identity are checked; duplicate keys/entries, unknown fields, incomplete or extra matrix entries, path escape, symlink/hardlink substitution, placeholders, digest/size mismatch, license mismatch, wrong architecture/version/revision, or untrusted signatures are rejected.
- The inventory validator hands verified production artifacts into the same host runtime verifier and registrar used by installation. Native acceptance stages the verified signed inputs and registers them through the privileged runtime catalog; a JSON file alone cannot qualify installation.
- Required **acceptance/p036/production-artifacts.json**, its signature, artifact files and publisher trust material are absent. The validator reports **BLOCKED_BY_EXTERNAL_INPUT**; CI is expected to stop at that gate. No dummy package, unsigned runtime, placeholder signer or fabricated SHA-256 was added.
- Required public trust configuration is documented in **docs/v0.2/HOST_RUNTIME_INPUTS.md** and the workflow: **SYNVEIL_P036_TRUSTED_KEYRING_B64**, **SYNVEIL_P036_INVENTORY_FINGERPRINT**, **SYNVEIL_P036_DESKTOP_FINGERPRINT**, **SYNVEIL_P036_SERVER_FINGERPRINT**, **SYNVEIL_P036_POSTGRESQL17_FINGERPRINT**, **SYNVEIL_P036_EDGE_FINGERPRINT**. These are approved public trust inputs, not substitutes for artifact signatures.

### Native acceptance harness and data recovery

- Added executable, bounded **scripts/run-p036-native-acceptance.sh** and **scripts/run-p036-power-interruption.sh** with real Python/Rust acceptance implementations. They reject unsupported OS/architecture, missing root-owned disposable marker, insufficient systemd/sudo/session prerequisites, missing signed artifacts, wrong PostgreSQL major, foreign runtime/data directories, or a VM that fails its marker/UUID/name/disk-root/SSH-host-key checks.
- The supported host runner labels are **[self-hosted, linux, x64, synveil-ubuntu-24.04-p036]** and **[self-hosted, linux, x64, synveil-fedora-42-p036]**. They require disposable Ubuntu 24.04/Fedora 42 x86_64 systems with systemd, sudo, package/build prerequisites, an active graphical user session and polkit agent for normal authorization, and the root-owned **/etc/synveil-p036-disposable** marker. The VM runner label is **[self-hosted, linux, x64, synveil-p036-vm-control]**; it requires a marked disposable libvirt VM, UUID-pinned domain, disk paths confined to approved disposable roots, pinned SSH known_hosts and a private SSH identity.
- The implemented journey executes actual verified artifacts and existing Host owner/coordinator/bootstrap/controller code for fresh installation, dependency checks, storage selection, safe PostgreSQL setup, local HTTPS, systemd, first administrator, normal authentication, first Library, initial sync using a real generated file, and readiness evidence. Snapshot comparisons cover server/storage/database identity, administrator and credential preservation, service/configuration state and server data across restart, repair, reinstall and normal uninstall. Repair reconciles existing state rather than resetting it. Power interruption validates state after hard VM stop/start. This is a real harness design, not passing execution evidence.
- The Rust acceptance path invokes coordinator/controller interfaces rather than driving the graphical wizard UI. Therefore **full GUI wizard journey: NOT_EXECUTED** even if a host runner becomes available; GUI automation/native user-flow acceptance remains a separate gap. No successful mock responses are treated as Host acceptance.
- Local execution confirmed fail-closed outcomes: missing inventory returns **BLOCKED_BY_EXTERNAL_INPUT**; missing disposable VM marker returns **BLOCKED_ENVIRONMENT**. Native Ubuntu/Fedora journey and controlled interruption were **NOT_EXECUTED**.

## C. Root cause and outcome: R01 PostgreSQL workflow failure

R01 exact-head PostgreSQL workflow [#39171826358](https://github.com/nghianguyen150612/Synveil/actions/runs/39171826358) failed in **runner_scheduler_failure_boundary** at **crates/metadata/tests/scheduled_maintenance_cycle_runner_postgres.rs:924**. The fixture used two independent clock_timestamp() calls for effective_from and updated_at. That could make effective_from later than updated_at, violating the existing **backup_schedules_effective_time_order** CHECK before the intended application-level timezone corruption reached the runner. This was an invalid test fixture, not evidence of an application, schema, or migration regression.

R02 commit **0bed7c034aca35d5db51df001f6067a49867742b** sets both times deterministically after effective_from by one microsecond. The database CHECK stays enabled and valid; only the application-level corrupt timezone state is exercised. Focused live PostgreSQL 17 test: **PASS_SIMULATED, 1/1** against a disposable official PG17 container. The 0bed7c source snapshot below recorded the workflow as queued. The later exact-head run on 66e2cee passed overall and passed the runner failure boundary; the Windows test cleanup follow-up now requires a fresh exact-head PostgreSQL run before that evidence is carried forward. No production constraint was relaxed.

## D. Source verification and evidence

### Local checks on source-validation SHA 0bed7c034aca35d5db51df001f6067a49867742b

| Check | Result | Evidence and scope |
|---|---|---|
| cargo fmt --all -- --check | PASS_STATIC | Rust 1.99.1 toolchain. |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS_STATIC | Rust 1.99.1; after moving a worker health helper ahead of its test module to satisfy current Clippy. |
| cargo deny check | PASS_STATIC | advisories, bans, licenses and sources passed; existing duplicate-version notices remain visible. |
| cargo test --workspace --locked | PASS_SIMULATED | Exit 0 in Debian 13 Rust 1.99.1 container. PostgreSQL-only tests without SYNVEIL_TEST_DATABASE_URL were ignored. AppImage integration cases used UID 1000 to satisfy their root rejection; other tests retained their required root context. This was before the behavior-neutral helper move; focused host tests and Clippy were rerun after it. |
| cargo test -p synveil-server-service --locked | PASS_SIMULATED | 5/5 focused service tests. |
| cargo test -p synveil-server-host --locked | PASS_SIMULATED | 2/2 focused Host tests after helper move. |
| cargo check -p synveil-server-host --features p036-acceptance --bin synveil-p036-native-acceptance --locked | PASS_STATIC | Acceptance binary compiles. |
| python3 -m unittest scripts.tests.test_p036_artifact_inventory -v | PASS_SIMULATED | 10/10 validator/schema negative and positive fixture tests; fixtures are synthetic unit fixtures, not production artifact evidence. |
| Python compile and shell syntax checks | PASS_STATIC | New validators/harness modules py_compile; build and native wrapper scripts bash -n. |
| python3 scripts/validate-server-service-installation.py | PASS_STATIC | Service/ownership/install contracts. |
| python3 scripts/validate-end-to-end-self-host.py | PASS_STATIC | Composition checks pass; Phase-E qualification is explicitly withheld. |
| bash scripts/validate-docs.sh | PASS_STATIC | 23 documentation validation units. |
| systemd-analyze verify, all six units | PASS_STATIC | Workflow-equivalent fixture executables; no service was started. |
| scripts/validate-p036-production-artifacts.py --platform ubuntu-24.04 | BLOCKED_EXTERNAL_INPUT | Required production-artifacts.json and authenticated production bundle absent; exit 2. |
| scripts/run-p036-native-acceptance.sh ubuntu-24.04 | BLOCKED_EXTERNAL_INPUT | Required signed inventory/artifacts absent; exit 2. |
| scripts/run-p036-power-interruption.sh | BLOCKED_ENVIRONMENT | Root-owned disposable-runner marker and provisioned VM absent; exit 2. |
| git diff --check | PASS_STATIC | Clean whitespace/conflict-marker validation. |

The full native platform requirements were unavailable in this execution environment. No Ubuntu 24.04/Fedora 42 service start, native package installation journey, graphical wizard, reboot, power interruption, or systemd health transition was observed.

### Exact source-head GitHub Actions snapshot

Fetched for source SHA **0bed7c034aca35d5db51df001f6067a49867742b** at **2026-10-19 02:03 UTC**. This is the PR source-validation head, before documentation-only closeout. The snapshot had two passed workflows, one failed-at-missing-input workflow, four in progress, and fourteen queued. An empty combined commit status was returned; workflow results below are the individual Actions runs, not green branch protection.

| Workflow | Run | Exact observed state and classification |
|---|---|---|
| Prompt 034 server network reachability | [#39187654956](https://github.com/nghianguyen150612/Synveil/actions/runs/39187654956) | IN_PROGRESS. |
| Windows Installer | [#39187655040](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655040) | IN_PROGRESS. |
| Connection setup simplification | [#39187655014](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655014) | QUEUED. |
| iOS Build | [#39187655029](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655029) | PASS. |
| Library first-run wizard | [#39187655025](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655025) | QUEUED. |
| Prompt 035 first-admin bootstrap | [#39187655028](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655028) | QUEUED. |
| Linux AppImage | [#39187655063](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655063) | FAILED at required type-2 runtime and independently trusted SHA input; static checks passed, artifact inspection/smoke skipped. **BLOCKED_EXTERNAL_INPUT**, not a green build. R03 handoff. |
| iOS Rust Apple Build | [#39187655060](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655060) | IN_PROGRESS. |
| PostgreSQL 17 scheduled maintenance | [#39187655094](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655094) | QUEUED; repaired scheduler fixture has not yet passed this exact-head workflow. |
| Installation resilience hardening | [#39187655091](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655091) | QUEUED. |
| Rust CI | [#39187655178](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655178) | QUEUED. |
| Server self-host acceptance / P036 | [#39187655148](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655148) | All contract, inventory, Ubuntu, Fedora and interruption jobs QUEUED at snapshot. No native runner capability is inferred from queue state. Inventory is absent, so artifact identity is expected to block when it runs. |
| Linux DEB/RPM | [#39187655126](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655126) | Qt reproducibility job IN_PROGRESS; package build and systemd 249 jobs QUEUED. |
| Repair and recovery UX | [#39187655181](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655181) | QUEUED. |
| Authentication UX polish | [#39187655117](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655117) | QUEUED. |
| Prompt 033 server services | [#39187655134](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655134) | PASS_STATIC; static-contract job success, including unit verification with parser fixtures. No services started. |
| Installer security hardening | [#39187655175](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655175) | QUEUED. |
| iOS Simulator Tests | [#39187655155](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655155) | QUEUED. |
| Installation-to-sync progress | [#39187655143](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655143) | QUEUED. |
| Linux clean-machine acceptance | [#39187655132](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655132) | Contract/artifact build jobs QUEUED. R04 dependency; not qualified. |
| Windows native acceptance | [#39187655188](https://github.com/nghianguyen150612/Synveil/actions/runs/39187655188) | Candidate build IN_PROGRESS. GUI/new-logon/named-pipe remains R04 native evidence; no result yet. |

These are source-head results, not final documentation-head workflow results. The PR source head at the time of this snapshot was **0bed7c034aca35d5db51df001f6067a49867742b**. R01's earlier exact-head results and external blockers remain in [RELEASE_HARDENING_R01_MANIFEST.md](RELEASE_HARDENING_R01_MANIFEST.md); they are not rewritten or closed by R02.

## Final PR-head workflow follow-up and Windows fixture repair

The documentation closeout head **66e2ceed22740339bad1bb607e1c0c94313319c0** was checked again at **2026-10-19 02:32 UTC**. This exact head is stacked on the same verified R01 base. Its workflow evidence is recorded separately from the earlier 0bed7c source snapshot above.

| Workflow | Run | Exact observed state |
|---|---|---|
| Server self-host acceptance / P036 | [#39188222635](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222635) | Not terminal. Contract/source passed; artifact identity failed closed because the approved public publisher keyring and fingerprints were absent; Ubuntu, Fedora and interruption jobs remained queued. Native acceptance did not run. |
| Prompt 034 server network reachability | [#39188222708](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222708) | PASS. |
| iOS Simulator Tests | [#39188222532](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222532) | PASS. |
| Linux clean-machine acceptance | [#39188222562](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222562) | FAILED at Build AppImage because the type-2 runtime and trusted SHA were absent; contract gate passed and downstream acceptance was skipped. BLOCKED_EXTERNAL_INPUT, R03/R04 dependency. |
| PostgreSQL 17 scheduled maintenance | [#39188222540](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222540) | PASS overall. All PG17 jobs passed, including Prompt 68 runner_scheduler_failure_boundary (step 30/50), which passed with the repaired schema-valid fixture. This closes the reported R01 scheduler test failure on head 66e2cee. |
| Installation resilience hardening | [#39188222512](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222512) | PASS. |
| Rust CI | [#39188222505](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222505) | IN_PROGRESS at snapshot; Test (windows-latest) failed while the remaining jobs were still running. The failure and follow-up are described below. |
| Installer security hardening | [#39188222497](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222497) | PASS. |
| Windows Installer | [#39188222462](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222462) | IN_PROGRESS. |
| Windows native acceptance | [#39188222464](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222464) | IN_PROGRESS; no GUI, new-logon, or named-pipe qualification result yet. R04 remains open. |
| iOS Rust Apple Build | [#39188222603](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222603) | PASS. |
| Linux native packages (DEB + RPM) | [#39188222513](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222513) | IN_PROGRESS; systemd 249 validation passed, package and Qt reproducibility jobs still running. Artifacts remain unsigned CI outputs. |
| Prompt 033 server services | [#39188222448](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222448) | PASS_STATIC; parser validation passed, no service started. |
| Connection setup simplification | [#39188222443](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222443) | PASS. |
| Prompt 035 server first-admin bootstrap | [#39188222447](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222447) | PASS. |
| Installation-to-sync progress | [#39188222445](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222445) | PASS. |
| Linux AppImage | [#39188222570](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222570) | FAILED closed: exact type-2 runtime file and independently trusted SHA-256 are required before acquisition/build. Nine AppImage integration tests passed; build and artifact smoke were skipped. BLOCKED_EXTERNAL_INPUT, R03 handoff. |
| Library first-run wizard | [#39188222549](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222549) | PASS. |
| Repair and recovery UX | [#39188222460](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222460) | PASS. |
| Authentication UX polish | [#39188222551](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222551) | PASS. |
| iOS Build | [#39188222440](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222440) | PASS. |

#### Windows workspace test failure and repair

Rust CI run **#39188222505**, job **Test (windows-latest)**, ran 25 client-sync host tests: 24 passed and **host33_root_loss_fences_queued_observation_without_losing_durable_work** failed at fixture cleanup with Windows error 32 (“file is being used by another process”). The test shut down the host and dropped its observer/source, but kept the **DesktopSyncHost** value alive while removing the profiled fixture directory. That host still owned the configured filesystem replica. Unix permits removing an open directory tree, which hid the missing test teardown; Windows correctly refused it. This is a test fixture cleanup defect, not a production database/storage overwrite or a service behavior change.

R02 now drops the host before fixture cleanup in host33 and the related host35 multi-library case. Rust formatting passed; the two focused tests passed locally, 1/1 each. The exact Windows CI rerun on the new follow-up head is pending, so Windows CI is not yet marked fixed. The full relevant Windows native GUI/new-logon/named-pipe gate remains the separate R04 acceptance.

## E. Remaining external inputs and acceptance blockers

1. **Authentic managed runtimes:** Provide version-pinned production PostgreSQL 17 and Caddy runtime payloads for Ubuntu 24.04/Fedora 42 x86_64, signed runtime manifests and detached signatures, closed payload inventories, exact upstream source revision/source URL, version probes, license and notice bytes, and approved independent publisher public keyrings/fingerprints. Provide a signed server package input from the approved build process. The runtime registration scripts then verify and bind the bytes; do not fetch arbitrary executables or provide operator-invented checksums.
2. **P036 artifact set:** Supply actual desktop DEB/RPM and server/PostgreSQL/Caddy signed runtime manifests and referenced payloads for both platforms, exact paths and sizes, independently calculated SHA-256, inventory signature, provenance/license/notices, and approved signer fingerprints/keyring. Populate the exact eight entries accepted by schema v1; run validator and Host registrar against those exact files. Missing inventory currently returns **BLOCKED_BY_EXTERNAL_INPUT**.
3. **Repository trust configuration:** Supply the six public variables listed above from the actual publisher trust decision. Supply no private signing key to this validator. Any private build/release signing credentials must be handled by the approved external release system, not committed here.
4. **Native host runners:** Provision authorized, disposable runners with the exact Ubuntu/Fedora labels listed above, root/sudo and systemd, x86_64, package/build dependencies, graphical session and polkit agent, and correctly permissioned disposable marker. The actual production artifact set must also be available on those runs. Current queue state does not establish runner availability.
5. **Interruption VM:** Provision the labeled disposable libvirt runner and guest with root-owned marker, exact UUID/name, approved file-backed disk locations, SSH identity, pinned known_hosts, and a completed native Host journey. Current environment has no such marker or guest.
6. **GUI acceptance:** Add/execute a real graphical wizard automation on supported desktop runners. The implemented native harness invokes actual backend coordinator/controller paths; it does not automate the GUI wizard itself.
7. **Exact-head CI:** The exact-head PG17 run #39188222540 passed on 66e2cee, including the repaired runner failure-boundary test. The follow-up Windows fixture cleanup change requires fresh exact-head Rust and PG17 reruns; those results must be reviewed on the final pushed head. If R01 merges while PR #149 remains open, merge updated main into R02 with a normal merge commit, retarget PR #149 to main, and rerun affected validation. Do not use squash or history rewriting.

## F. R01/R03/R04/R05 handoff

- **R01:** Keep PR #148 open until its exact-head workflows are reviewed. The R02 scheduler fixture repair passed the full exact-head PG17 workflow [#39188222540](https://github.com/nghianguyen150612/Synveil/actions/runs/39188222540), including runner failure boundary; a subsequent client-sync test cleanup follow-up awaits its own exact-head PG17 rerun. Other R01 outcomes remain visible in the historical R01 manifest.
- **R03 AppImage:** Supply genuine AppImageKit type-2 **runtime-x86_64** bytes and an independently trusted SHA-256 using the existing **SYNVEIL_APPIMAGE_RUNTIME_FILE** and **SYNVEIL_APPIMAGE_RUNTIME_SHA256** contract. Rebuild/reproduce the exact artifact and verify its final artifact/manifest identity. Do not use a downloaded runtime with an unreviewed digest.
- **R03 DEB/RPM:** Build exact reproducible `.deb` and `.rpm` artifacts with source/provenance and independently verified final digests; supply approved package/repository signing keys and repository publication credentials. Current package workflow produces unsigned CI artifacts and does not prove repository release signing.
- **R03 Windows/signatures/manifests:** Supply approved Authenticode identity/private signing service, timestamp service, public trust bootstrap/fingerprints, and signing/rotation/revocation policy. Sign installer, payload executables and uninstaller; calculate manifest digests over final signed bytes. P036 inventory signer inputs and secret bindings must come from approved release operations.
- **R03 distribution:** Provide an approved distribution origin, authenticated channel-manifest publication, version and downgrade policy, repository/host ownership and credentials, retention/rollback procedure and smoke-test environment. No public distribution endpoint was contacted.
- **R04:** Execute Windows GUI, genuine new-logon and named-pipe acceptance on authorized Windows native runners. After R03 artifacts are ready, execute Ubuntu/Fedora clean-machine acceptance on the required supported runners. Keep both explicit until their native evidence is produced.
- **R05:** Remains not started. No production release qualification or public release is claimed by R02.

## G. Files changed

Implementation source commit **8a5d5c9** changed:

- .github/workflows/server-self-host-acceptance.yml
- .github/workflows/server-services.yml
- Cargo.lock
- acceptance/p036/production-artifacts.schema.json
- crates/api/src/bin/synveil-api.rs
- crates/api/src/bin/synveil-server-migrate.rs
- crates/api/src/bin/synveil-worker.rs
- crates/server-host/Cargo.toml
- crates/server-host/src/bin/p036-native-acceptance.rs
- crates/server-host/src/native/services.rs
- crates/server-service/Cargo.toml
- crates/server-service/src/native.rs
- crates/server-service/src/runtime.rs
- deploy/server/POSTGRESQL_RUNTIME_ARTIFACT.md
- deploy/server/prepare-host-runtime.py
- deploy/systemd/synveil-edge.service
- deploy/systemd/synveil-postgresql.service
- docs/v0.2/HOST_RUNTIME_INPUTS.md
- docs/v0.2/SERVER_SERVICE_INSTALLATION.md
- scripts/build-host-server-input.sh
- scripts/materialize-p036-trust-keyring.py
- scripts/p036-snapshot-state.py
- scripts/p036_native_acceptance.py
- scripts/p036_power_interruption.py
- scripts/requirements-p036.txt
- scripts/run-p036-native-acceptance.sh
- scripts/run-p036-power-interruption.sh
- scripts/tests/test_p036_artifact_inventory.py
- scripts/validate-p036-production-artifacts.py
- scripts/validate-server-service-installation.py

Scheduler fixture commit **0bed7c0** changed **crates/metadata/tests/scheduled_maintenance_cycle_runner_postgres.rs**. Follow-up commit **045dc1e** changed **crates/client-sync/src/host.rs** for Windows fixture teardown and updated **docs/v0.2/RELEASE_HARDENING_R02_MANIFEST.md** and **docs/v0.2/RELEASE_HARDENING_TRACKER.md**.

## Final Git and qualification state

- Final branch: **feat/v02-managed-host-production-readiness**.
- R02 PR: **#149**, target **fix/v02-release-ci-stabilization**, open and unmerged at documentation closeout. PR URL: https://github.com/nghianguyen150612/Synveil/pull/149.
- R01 PR #148 remains open and unmerged; R01 merge SHA: **none**. R02 merge SHA: **none**.
- Source implementation validation SHA: **0bed7c034aca35d5db51df001f6067a49867742b**. Windows test cleanup follow-up source SHA: **045dc1e17cf6e980bc83ceca7c77a830e967ea28**. The final documentation closeout SHA is reported in the final handoff; a commit cannot contain its own hash.
- Final R02 status: **R02_SOURCE_COMPLETE_WITH_BLOCKERS**.
