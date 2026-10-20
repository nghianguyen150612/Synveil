# Synveil v0.2 release hardening R04 manifest

## Closeout classification

**R04_SOURCE_COMPLETE_WITH_BLOCKERS.** The confirmed Linux observer overflow
defect has a durable, fail-closed repair and deterministic regression coverage.
The native acceptance workflows now bind manual Windows and Linux runs to the
exact assembled candidate, reject source or byte mismatches, and retain
candidate provenance. The P045 native matrix and P047 release qualification
remain blocked because this stacked R04 revision cannot yet produce its own
release candidate and the required production and native inputs are absent.
No v0.2.0 tag, GitHub Release, stable channel, or merge was created.

## A. Starting state and Git integration

- Repository: `nghianguyen150612/Synveil`.
- Verified `origin/main`: `b01c3b2328b271529d02cb4ee30d97be43a42611`.
- R01 PR [#148](https://github.com/nghianguyen150612/Synveil/pull/148):
  open at `24f84d6e9460d82380f3ab017a3a2d36d24f2879`, based on `main`.
- R02 PR [#149](https://github.com/nghianguyen150612/Synveil/pull/149):
  open at `f6f2591f44caf54ff13de2b0fd869257674de655`, based on R01.
- R03 PR [#150](https://github.com/nghianguyen150612/Synveil/pull/150):
  open at `524ef926324a0e965735bf2097d67792095a525a`, based on R02.
- R04 branch: `release/v02-native-acceptance`.
- R04 integration base: `release/v02-production-artifact-readiness`.
- Initial R04 parent/source: `524ef926324a0e965735bf2097d67792095a525a`.
- R04 implementation commit: `2488d6ea20856d1633512dab3b77e55bb630c639`
  (`fix(sync): preserve intent after watcher overflow`).
- The follow-up acceptance-workflow correction is
  `17c70d870fbdd743494469eb97b09b1541a5bfc1`; it checks Authenticode status
  on the downloaded Setup path before candidate staging.
- R04 PR [#151](https://github.com/nghianguyen150612/Synveil/pull/151) is
  stacked on R03. It remains open; no prerequisite PR has merged. The exact
  branch head and later report-only commits are listed in the tracker.

The branch preserves the existing P001–P060 history. Historical P045 PR #78
and P047 PR #104 remain closed and unmerged records; their original evidence
and failures have not been rewritten.

## B. Observer regression and repair

R03's Repair and Recovery UX run
[39266772357](https://github.com/nghianguyen150612/Synveil/actions/runs/39266772357)
failed `observation::tests::linux_notify_backend_handles_live_rename_move_delete_symlink_and_editor_save`
in `crates/client-sync/src/observation.rs` (292 passed, 1 failed).

The failure is a real watcher-overflow safety defect, not a timeout failure or
an incorrect expectation. The bounded watcher queue dropped part of a Linux
remove/create pair during a move. The old path still represented a tracked
node while the new path appeared as a create. Reconciliation could then
persist a delete for the tracked node and a separate create for the new path.
When the event buffer overflowed, buffered hints no longer represented a
complete event history, but the observer could still flush those hints before
the full scan completed.

The repair in `crates/client-sync/src/observation.rs` clears all buffered
hints on overflow and relies on a full-tree reconciliation. When an unseen
tracked node and a same-kind create intent coexist, reconciliation records a
durable `AmbiguousRename` at the source and destination and does not infer a
delete. `crates/client-sync/src/state.rs` moves an already durable,
unsubmitted delete intent to `BLOCKED`; it retains that intent and the new
create intent. The observation issue continues to fence outbound submission.
The repair does not disable watcher events, discard user intent, or relax
symlink restrictions.

The exact-candidate Windows workflow review found a separate harness wiring
defect: Authenticode state was checked using the later staged directory before
the downloaded Setup had been copied there. Commit
`17c70d870fbdd743494469eb97b09b1541a5bfc1` points the check at
`target/release-candidate/SynveilSetup.exe` and extends
`scripts/validate-windows-native-acceptance.py` to assert that ordering and
path. The static Windows acceptance contract passed after the correction.

Coverage now separates live rename/safe ambiguity, atomic editor-save/delete,
and symlink behavior. A deterministic overflow test asserts that source
deletion remains blocked, destination creation remains pending, and ambiguity
is durable at both paths. The unpaired remove/create test also checks both
locations.

## C. Exact release candidate identity and Windows ZIP investigation

The latest product-and-acceptance source under review is commit
`17c70d870fbdd743494469eb97b09b1541a5bfc1`; product version is `0.2.0`.
Commit `2488d6e` contains the observer repair, and `17c70d8` adds the
Windows acceptance-path correction. R04 candidate artifacts were not
produced. The authoritative R03 `release-artifact-candidate.yml` exists on the
stacked R03/R04 branch but not on the repository default branch, so GitHub
returns 404 for `workflow_dispatch` until R03 is integrated. The R04 consumer
workflows correctly require a successful candidate producer run and reject
an older source SHA. The R03 test-merge candidate at
`7f88d114e272ac220056db216030acda84be90e1` therefore cannot be substituted.

The exact older R03 artifact identities and separate GitHub Actions archive
identities remain recorded in
[RELEASE_ARTIFACT_MATRIX.md](RELEASE_ARTIFACT_MATRIX.md). They are historical
unsigned candidate evidence only and are not R04 acceptance subjects.

### Windows portable ZIP discrepancy

R03 selected Windows ZIP: 51,621,495 bytes,
SHA-256 `139ed73ff48eff11198a9c73a28b10b086e25455fcbffbabf4ca4241a83a772b`.
The separate Rust CI ZIP: 51,620,638 bytes,
SHA-256 `51c054eb14ead62c790528c69872bf88e7d936a52eb28c3db0421d7ebf53801`.
The difference is 857 bytes. Both were reported at the same test-merge source
and used `build-windows.sh`; existing R03 evidence records normalized archive
timestamps, member ordering, compression settings and ZIP metadata, the same
Python 3.14.7 ZIP writer and Qt 6.8.3. The package manifest embeds source
revision/fingerprint, toolchain, Qt, `windeployqt`, and MSVC runtime identity;
the runtime closure is also produced by `windeployqt`.

The original Actions archives were requested again during R04, but GitHub's
artifact blob endpoint returned HTTP 403. The contained members and embedded
manifests therefore could not be independently extracted and compared. No
unsupported claim is made that the old discrepancy has a proven root cause.
The current producer change makes the selected Windows installer workflow
build the selected ZIP twice in the same environment and fail unless both the
ZIP and package manifest are byte-identical. This establishes same-producer
reproducibility for a future R04 candidate; it does not retroactively explain
the old cross-workflow divergence. The discrepancy remains a release blocker
until exact R04 producer evidence or accessible source artifacts establish the
member-level cause and the chosen release source is independently reproduced.

The documented production source for the next candidate is the Windows job in
`release-artifact-candidate.yml`, which calls
`windows-installer.yml`; that producer now emits and reproduces the selected
ZIP. Rust CI output is a separate diagnostic subject and cannot be silently
substituted.

## D. Native environments actually used

The local executor is a Debian 13 container, UID 1000, with no graphical
display, Windows session, Ubuntu 24.04 or Fedora 42 clean desktop, or real
logon transition. Local Docker PostgreSQL 17 was used only for the database
migration integration gate. It is not clean-machine GUI or native package
acceptance.

The R04 PR workflows ran on GitHub-hosted Linux, macOS and Windows images. The
hosted Windows image is not Windows 11 with an interactive standard-user
desktop, genuine logon transition and named-pipe session evidence. The Linux
clean-machine PR path skips its native guest matrix; only `workflow_dispatch`
consumes a verified exact candidate. A workflow-level success or skipped
matrix is not a native PASS.

The self-hosted P036 runner inventory endpoint returned HTTP 403 to the
available GitHub integration, so the required labels could not be verified.
The exact P036 labels are `synveil-ubuntu-24.04-p036`,
`synveil-fedora-42-p036`, and `synveil-p036-vm-control`. The historical P045
handoff in closed, unmerged PR #78 calls for the disposable Linux KVM label
`synveil-p045-kvm` and a separate interactive Windows 11 VM; neither runner
identity was available in this environment. The repository Actions runner API
request failed with HTTP 403, so label presence is unknown rather than assumed.
The P036 workflow's artifact identity gate fails closed because
`acceptance/p036/production-artifacts.json`, its detached signature and the
approved production keyring/fingerprints are absent. Its queued self-hosted
jobs are not native results.

## E. Upgrade, rollback and data preservation

Both `deploy/release/native-upgrade-policy.json` and
`deploy/release/windows-upgrade-policy.json` retain an empty `upgrade_from`
array. No prior version, including v0.1.x, is approved for automatic upgrade.
Same-version fixture repair and synthetic upgrade/downgrade checks are not
evidence for a supported source-to-target release path.

No qualified native test ran for upgrade, rollback, process interruption,
disk-full recovery, real database migration from a prior release, credential
reconciliation, or power interruption. PostgreSQL user data, object-store
identity, client library identity, durable sync queues/checkpoints, and
Keychain boundaries were not reset or rotated. A successful rollback is not
claimed because original protected state and compatible binaries were not
restored in a native rollback journey.

The PostgreSQL 17 empty-database migration gate passed 2 tests and applied 36
migrations; the fail-closed future/missing-table case preserved its evidence.
This validates migration behavior on a disposable database, not preservation
of an installed user's database during product upgrade or recovery.

## F. Validation performed

- `cargo fmt --all -- --check`: passed.
- `cargo test -p synveil-client-sync --locked --lib observation::tests::`:
  22 passed.
- `cargo test -p synveil-client-sync --locked --lib`: 296 passed.
- `cargo clippy -p synveil-client-sync --all-targets --locked -- -D warnings`:
  passed.
- `cargo deny check`: passed; existing duplicate dependency warnings remain.
- `cargo clippy --workspace --exclude synveil-desktop --all-targets --locked -- -D warnings`:
  passed.
- `cargo test --workspace --exclude synveil-desktop --locked`: passed. The
  suite's database tests that require an external PostgreSQL URL remained
  ignored by their explicit opt-in contract; the separate PostgreSQL 17
  migration gate ran and passed 2/2 tests.
- PostgreSQL 17 migration gate: 2 passed.
- Release candidate provenance verifier unit tests: 6 passed.
- Windows installer and native acceptance static contract validators: passed;
  native checkpoint remains blocked.
- Windows manual-candidate signature-path regression assertion and workflow
  YAML parse: passed.
- Release manifest, download integrity, channel, installation acceptance,
  installer engine/security, P036 inventory/signing, AppImage static, and
  documentation validators passed locally as recorded in the R04 report.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` and
  `cargo test --workspace --locked` cannot complete in this local container
  because Qt is unavailable. The workspace checks excluding
  `synveil-desktop` passed as recorded above.
- `scripts/validate-release-artifacts.sh` is blocked because no R04 generated
  release artifact manifest exists locally.

Exact-head hosted workflow outcomes and any failures or skips are listed in
the tracker and
[RELEASE_CANDIDATE_QUALIFICATION_R04.md](RELEASE_CANDIDATE_QUALIFICATION_R04.md).

## G. R05 handoff

R04 is not eligible for P048. There is no qualified R04 artifact set, no
approved signer/trust identity, no P036 production inventory, no native
Windows 11 / Ubuntu 24.04 / Fedora 42 result, no authenticated distribution
origin, and no approved upgrade source. After R01–R03 merge in order, R05 must
rebuild from the final integrated source, capture final-byte hashes and
signatures, resolve the Windows ZIP discrepancy with member-level evidence,
execute P045/P036 on authorized native runners, pass all 21 P047 gates, and
revalidate any artifact affected by subsequent source changes.
