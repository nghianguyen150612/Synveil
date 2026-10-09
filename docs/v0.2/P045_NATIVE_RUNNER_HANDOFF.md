# P045 native runner handoff

P045 remains BLOCKED. Continue `feat/cross-platform-clean-machine-matrix` and draft PR #78. Do not merge, start P046, or tag v0.2.0. This handoff supplies execution requirements, not native acceptance results.

## Recorded failures and fixes

Checkpoint c4a872d6a92991578562aadd5faf215ee5ada925, P045 run 37925347518:

- Fedora RPM job 113831186422: DNF5 rejected `environment install` at guest uptime 112.586 seconds. AppImage job 113831186535 failed identically at 102.040 seconds. The provisioner now uses `dnf -y install @workstation-product-environment`, explicitly installs the X11 session/server, records its exit status, and stops readiness polling on provisioning failure. SSH probes are bounded. Shell arguments are JSON-encoded YAML scalars. Desktop readiness still requires the marker, user bus, active logind X11 session, and a working display query.
- Ubuntu DEB job 113831186453 was still unpacking desktop dependencies at uptime 1010 seconds; AppImage job 113831186686 at 984 seconds. There is no completed desktop or first Ubuntu cloud-init failure in those artifacts. Keep the current 900-second readiness bound; use hardware virtualization rather than guessing a larger timeout.
- Historical ff77ad5 run 37895485222, Fedora artifact 11602454793: `No match for argument: Fedora Workstation` at 216 seconds, plus cloud-init failures. Its unconditional later marker allowed scenario execution despite provisioning failure. Completed historical jobs do not demonstrate a working desktop.
- Windows Installer run 37925347193 / job 113802837165: lifecycle child failed `LIFECYCLE_TIMEOUT: repair-production`. The bounded Setup log records successful payload copy/registration at 12:03:54.870, but no completed post-install reconciliation. The token SID, non-admin status, and corrected token-profile paths passed. RestartManager found no package files in use. The target ownership manifest was previously parsed and path-validated for every old file in the Pascal interpreter. It is now authenticated and parsed once, with identity reauthentication before each obsolete-file deletion, and explicit post-install stage logs. Treat this performance cause as a hypothesis until Windows execution completes. A repeated actual same-version repair retains all preservation assertions. No timeout or privilege boundary is relaxed.
- Rust run 37925347128 / Windows Qt job 113802839105: 287 passed, one failed; immediate fixture cleanup hit Windows sharing violation 32 at local_migrations/tests.rs:207. A borrowed pooled migration connection was returned through SQLx's scheduled release task, which can race a one-connection pool close. Migration validation now holds the connection and awaits its worker close on failure. Fixture connections close explicitly; each rejected schema must release its database for immediate rename, twice, while preserving schema and profile assertions. No sleep, schema repair, or ignored cleanup error is added.

- The first resume head's Windows workspace run 37962021826 / job 113926963813 passed client-sync and then exposed 66 failing installer-journal cases (19 passed). They failed at journal creation before product effects. Directory durability used File::open without FILE_FLAG_BACKUP_SEMANTICS, and committed checkpoints were reopened read-only although Windows FlushFileBuffers requires writable handles. The follow-up opens checked, non-reparse directory handles with backup semantics and write access, and uses writable non-following handles to flush committed objects. Every durability failure still propagates; no directory/file flush is skipped. A fresh append/reopen regression and the full journal fault suite require native Windows execution; Linux and Windows-target compilation alone do not establish Windows durability or power-cycle qualification.

- First resume Windows Installer run 37962021747 / job 113926964347 confirms that both production repairs completed. The next failure is at lifecycle line 143: Remove-Item on a nonempty package root prompts in NonInteractive mode. The follow-up waits for both HKCU registration removal and every remaining package object except the explicitly preserved adjacent user file. It never deletes leftovers; persistent files or directories fail the bound with diagnostics. Root removal now propagates errors. Negative fixtures reject residual payload, residual directories and residual registration, and verify the preserved note's hash. The full standard-user native lifecycle must still validate uninstall completion.

## Minimum worker arrangement

Use one isolated Linux KVM host for disposable Ubuntu/Fedora guests and a resettable server VM, plus one isolated Windows 11 VM controlled by its hypervisor host. Worker registration alone does not establish a graphical session or hard-power control. No external machine has been installed or registered by this task.

For serialized Linux consumers, reserve at least 4 hardware CPU threads, 8 GiB RAM, and 80 GiB disposable storage. Each existing QEMU guest uses 2 vCPUs, 4 GiB RAM, and a 24 GiB persistent qcow2 overlay. For four simultaneous consumers, reserve 8 CPU threads, 24 GiB RAM, and at least 160 GiB storage. Allow package/image download time and retain bounded serial/QEMU diagnostics. Use a fresh per-job state directory and loopback-only SSH forwards/QMP Unix sockets. Grant the runner account access to `/dev/kvm` through its administrator-configured kvm group; verify `test -w /dev/kvm` before registration. Provide QEMU x86, qemu-utils, cloud-image-utils, genisoimage/xorriso, openssh-client, socat, Python, curl, and checksum tools. The existing job installs its prerequisites through sudo; use a disposable dedicated worker with the reviewed narrow administration policy needed for that step.

Only a repository administrator should register an ephemeral runner with labels `self-hosted,linux,x64,synveil-p045-kvm`. Route reviewed workflow_dispatch executions to it; ordinary PR execution retains hosted defaults. Do not grant untrusted PR code access to persistent workers, production networks, host secrets, or other jobs' disks. Use existing Actions token permissions (contents/actions read), pinned action versions already in the repository, and per-run synthetic guest credentials. Never upload guest passwords, private SSH keys, server credentials, or raw credential-bearing screenshots.

The two Linux images remain pinned in deploy/acceptance/images.lock:

- Ubuntu 24.04 x86_64: 6a81c37564db9b1ee84e141922625e1d7c5b389b99bb3c572e0243607d5bb4d2.
- Fedora 42 x86_64: e401a4db2e5e04d1967b6729774faa96da629bcf3ba90b67d8d9cce9906bec0f.

After provisioning the worker and reviewing the published head, run the existing matrix once:

```bash
gh workflow run cross-platform-clean-machine.yml \
  --repo nghianguyen150612/Synveil \
  --ref feat/cross-platform-clean-machine-matrix \
  -f 'linux_native_runner=["self-hosted","linux","x64","synveil-p045-kvm"]'
```

The new optional dispatch/reusable-workflow input changes only Linux native consumer placement; producer and source gates remain separate. Observe the resolved source SHA, producer hashes, KVM launch, successful cloud-init, genuine desktop readiness, and each typed scenario. Ubuntu/Fedora native GUI routes must use their declared package handler; a provisioned GNOME session does not establish the scenario's package handler, installed product, credentials, or first-run server.

## Windows 11 interactive qualification

Reserve 4 vCPUs, 8 GiB RAM, a 64 GiB disposable OS disk, and hypervisor-owned snapshots. Record actual Windows 11 edition, version/build, AMD64 architecture, image identity, and VM identity. Keep a separate administrator provisioning account and a synthetic standard-user account. Log the standard user onto an active console/RDP desktop that remains unlocked and attached throughout GUI execution. A Windows service-session runner or Start-Process -Credential child does not constitute that logon.

Transfer the exact published Setup and producer manifest to the VM; independently recompute SHA-256 and size before use. Launch `SynveilSetup.exe /LOG=<scenario-owned-path>` interactively from the standard user's session, observe actual Setup choices/consent, install location, non-elevated token, package ownership and user-only integrations. Start the installed desktop and installed client in that same profile. Observe real named-pipe IPC and normal product controls, including refusal of a second writer and an unrelated user's access. Neither a silent Setup nor fixture/offscreen IPC closes this gate. Capture sanitized GUI observations and endpoint ownership/access diagnostics. Exercise actual logout/login with explicit startup consent, repair, uninstall, reinstall and durable data/credential preservation, then delete the synthetic account and revert the disposable VM.

The existing `invoke-windows-standard-user-test.ps1` and native lifecycle scripts can supply scoped installation/cleanup diagnostics. They cannot currently emit the missing interactive/logon/installed-pipe acceptance evidence. Complete that reviewed driver on the interactive VM before producing qualified result-v1 records; do not fill its missing assertions by hand. Keep the Windows row BLOCKED until all six required scenarios meet their contracts.

## Genuine interruption journey

INSTALL-JOURNEY-8 requires all declared points: acquisition, package_payload_mutation, platform_integration, verification. Keep the same persistent guest disk before and after each interruption. A clean or installed snapshot is the starting condition; do not restore the snapshot after the cut and discard the mutation being tested.

A reviewed driver must observe an actual product mutation boundary through an authenticated observer/barrier, record disk/VM/artifact identity and journal state, then command the hypervisor to remove guest power without an OS shutdown (for example `virsh destroy <dedicated-disposable-domain>` or `Stop-VM -TurnOff` on the isolated hypervisor). Restart that same disk, inspect authoritative package/journal state, run the real reconciliation path, verify normal readiness and preservation fingerprints, and emit every required assertion and interruption-point observation. Bound waiting and archive sanitized pre/post diagnostics. Restrict hypervisor control to the named disposable domains; keep operator access separate from guest standard-user privileges.

The existing QEMU control plane and snapshots are useful orchestration, but its final host-process termination is not a completed mutation-boundary acceptance journey. Process SIGKILL, graceful shutdown, Docker restart, process restart and fixture recovery remain insufficient. No qualifying power-cycle was executed here; the boundary/recovery driver remains an explicit prerequisite.

## Controlled server and production identities

Provision a separate disposable server VM reachable from the actual guests, with restricted test-only network access, persistent scenario-owned storage, and authenticated TLS/service identities. Use fresh databases for bootstrap scenarios; reset by discarding only scenario-owned state between scenarios, never by reopening a completed production bootstrap. Use normal installed-client Connect/onboarding/credential/sync and Host flows, not source stubs or arbitrary DB URLs. Keep secrets in the worker's protected secret store and upload only references/redacted diagnostics.

The P036 release/dependency producer must supply acceptance/p036/production-artifacts.json for desktop, server, postgresql17 and edge. Required fields: version, source_revision, platform, architecture, sha256, provenance, license. Compute digests from actual distributed bytes and authenticate provenance against the exact release/CI producer and dependency publisher. Inventory, authentic production identities and reviewed native scripts run-p036-native-acceptance.sh / run-p036-power-interruption.sh are absent at this checkpoint; labels alone cannot run them.

Owning dependencies remain separate:

- server-first-admin-bootstrap.yml, run 37925347186, job 113802833081: auth Postgres test observed Closed but expected Open at crates/auth/tests/postgres.rs:57, consistent with reused/closed bootstrap state. Its owner must provide an isolated fresh-DB fixture before first-run qualification.
- postgres-17.yml, run 37925347242, job 113803140057: live_pg17_adversarial_crash_restart_handoff expected schema 7 but observed 8 at crates/api/tests/sync_adversarial_postgres.rs:995. This is a separate schema-expectation dependency, not evidence to alter client migration policy.
- server-self-host-acceptance.yml run 37925347225 remains queued on unavailable/unverified worker capabilities; it also references the absent production inventory and drivers. Registering runners will not create those prerequisites.

No unrelated server implementation is changed by P045. FIRST-RUN-1/2 remain BLOCKED until genuine candidate identities, controlled server state, installed UI flows and native evidence exist.

## Evidence and closure procedure

Use the repository's result-v1 schema and scenario definitions unchanged. Conceptual source/fixture/offscreen/scoped/qualified distinctions do not introduce new schema enums. Validate source SHA, exact producer bytes/manifest hash, target OS/build/architecture, scenario digest, producer and consumer identities, session/privilege/capability observations, completed steps, preservation and cleanup independently. Missing evidence stays BLOCKED; recorded product failures stay FAIL.

Run the existing aggregate against downloaded producer artifacts and consumer records:

```bash
python3 scripts/validate-cross-platform-clean-machine-matrix.py \
  --aggregate <downloaded-artifacts-root> --artifact-root <downloaded-artifacts-root> \
  --source-commit <published-40-character-head> --output <report-path> --require-pass
```

Report raw counts and qualified counts separately. Archive exact run/job/artifact IDs and authenticated hashes. Preserve failed records and bounded diagnostics. PR #78 stays OPEN/DRAFT/UNMERGED while any applicable requirement is non-PASS or insufficiently evidenced.
