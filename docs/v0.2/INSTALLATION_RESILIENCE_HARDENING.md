# P044 — Installation Resilience Hardening

P044 hardens interruption recovery within the existing P008/P008A transaction
journal, P006 acquisition, P009 lifecycle, Linux package, AppImage, and P027
Windows ownership boundaries. It does not create a second recovery log or
claim rollback where a platform cannot provide it. After ambiguous mutation,
the installer inspects authoritative state, reconciles it with durable evidence,
and then resumes, replans, repairs, or stops.

## Recovery rules

- A durable `EffectMutationStarted` checkpoint precedes every journaled
  `apply_effect`. If it cannot be committed, the adapter is not called.
- An unmatched start, interrupted compensation, or failed post-mutation journal
  append requires inspection. A missing child-process result is not proof of
  failure or success.
- An effect is skipped only when its `EffectVerified` evidence exists; the
  engine performs the required read-only verification on resume.
- A completed transaction returns `AlreadyCompleted` without new mutation.
  Final verification may run again; durable completion cannot precede it.
- A changed plan is replanned. Unknown schema, broken chains, ambiguous
  ownership, and newer versions fail closed.
- APT/DNF/dpkg/rpm remain the authority for native package state. Package
  manager lock files are never removed to force progress.
- Recovery uses the existing signature, channel high-water, downgrade,
  containment, reparse, executable, and ownership checks. Artifacts are
  reverified before reuse or last use.
- User configuration, credentials, client state, libraries, server
  configuration, databases, object data, explicit disabled startup choices,
  external dependencies, and unknown neighboring files are preserved.

## Failure and interruption matrix

`Automatic resume` means the next execution can proceed without guessing about
an ambiguous mutation. `Inspection` means authoritative state must be checked
before a retry or later mutation. Evidence names the level exercised here; a
deterministic fixture or process kill does not establish physical power-loss
durability.

| # / failure boundary | Fault type | Durable evidence / possible state | Safe recovery; automatic resume? | Preservation | Evidence level |
|---|---|---|---|---|---|
| 1. Release metadata acquisition | network error, truncation, invalid identity | No installation checkpoint; authenticated channel high-water may already have advanced | Retry with current/fresher authenticated metadata; no package mutation; yes after freshness checks | All installation and user state untouched | Source/unit acquisition contracts; no power-loss claim |
| 2. Artifact download before completion | process loss, bounded stream failure | Private incomplete temp bytes only | Discard owned partial bytes and restart authenticated bounded download; no range resume; yes | Existing destination preserved; no package mutation | Acquisition fixtures; process kill of downloader not executed |
| 3. Artifact staging before file fsync | injected ENOSPC/write failure | Owned temporary file may contain a prefix | Remove only owned temp if possible; fail as disk-full/staging; retry fresh; yes | Destination and unrelated files preserved | Deterministic ENOSPC fixtures |
| 4. Staging after file fsync, before promotion | injected promotion failure or process stop | Complete private temp, no committed destination | Discard owned temp and restart authenticated bounded download; no partial/range reuse; yes | Existing destination is not replaced | Deterministic promotion fixture; no staging process-kill evidence |
| 5. Promotion before parent-directory durability | injected directory sync failure | A complete final name may be visible, but acquisition did not return verified | Rehash exact selected size/digest before reuse; no package mutation until verified; inspection of file identity is required | Existing file is never clobbered; only owned temp cleanup | Deterministic ENOSPC and I/O-failure fixtures; filesystem power loss not proven |
| 6. Preflight before any mutation | a platform-proven capacity block, stale plan, cancellation | No mutation-start checkpoint | Replan after user action or cancel; yes, no recovery inspection required | All durable state untouched | Source/unit and preflight contracts; no universal exact-byte preflight is claimed |
| 7. Immediately before mutation-start checkpoint | process loss or checkpoint failure | Earlier verified records only; current apply has not run | Reopen exact journal; safe to retry/replan only after full chain validation; yes | No current-effect mutation occurred | Journal unit/fault fixtures |
| 8. After durable mutation-start, before apply | process loss | Unmatched start; mutation is not known to have run | Inspect adapter authority first; `VerifiedNotApplied` requires replan; no blind replay | User/server state remains outside effect ownership | Journal deterministic fault fixture; process restart test covers nearby post-apply boundary |
| 9. During platform mutation | process loss, native partial result, ENOSPC | Durable start; effect may be absent, partial, or applied | Reconcile with platform authority; no automatic replay | Preserve data and unknown adjacent files; scoped repair only | Adapter contracts/fixtures; no native package kill fixture |
| 10. After platform mutation, before effect verification | process loss | Durable start and possible platform mutation | Inspect then verify authoritative state; no automatic resume before reconciliation | Preserve durable user/server state | Journal process-interruption fixture for common engine; native interruption unavailable |
| 11. After successful verification, before `EffectVerified` | journal I/O failure | Mutation may be complete; start checkpoint remains unmatched | Reconcile and verify, then append evidence; inspection required | No compensation or rollback inferred | Journal post-apply disk-full fixture |
| 12. Between effects | process loss | Prior effects verified; next effect has no start record | Reverify prior effects read-only, then continue; yes if they remain valid | Only planned owned effects may proceed | Existing journal resume tests |
| 13. During compensation | process loss | `CompensationStarted` is durable; inverse may be absent, partial, or applied | Inspect compensation state; do not replay it automatically | No expansion beyond safe inverse | Existing P008A journal fixtures; forced compensation process kill unavailable |
| 14. After compensation, before `CompensationVerified` | process loss or checkpoint failure | Compensation may have completed but lacks durable verification | Inspection required; never assume installed state or replay inverse | Protected state remains protected | Existing P008A journal tests; injected post-compensation ENOSPC not separately exercised |
| 15. During final installation verification | read-only verification failure/process loss | All effect checkpoints may be verified; completion absent | Rerun read-only final verification; yes if authoritative state passes | No new mutation required | Existing journal final-verification fixtures |
| 16. After final verification, before `TransactionCompleted` | process loss or journal failure | Durable final verification, completion absent | Revalidate final state and append completion; no effect replay | User/server state preserved | Existing journal semantic-transition tests |
| 17. During repair | forced process termination after restoring the first damaged owned file | Trusted same-version manifest and registration remain; a later owned payload may still be damaged or partial | Start a new same-version `/REPAIR=1` Setup, validate all target-owned file hashes and integration, and continue only within the prior manifest scope | Test-owned configuration, credential, client, library, server-state sentinels, disabled startup, and unknown neighbor are asserted | Focused real-Inno P044 repair-interruption fixture added; hosted result pending, so no repair-interruption pass is claimed yet |
| 18. During upgrade | process loss or disk exhaustion | Verified target artifact; one or more package-owned files may be replaced | Inspect exact version/ownership; repair a supported compatible target; no blind older-binary rollback | Data schemas are not rolled back; all durable state and unknown files survive | Focused real-Inno P044 fixture added with a bounded synthetic payload; hosted result pending, so no Windows interruption pass is claimed yet |
| 19. During ordinary uninstall | process loss | Some owned integration/payload/registration may be removed | Reconcile remaining trusted owned state; resume ordinary removal only; never turn into purge | Application/server data and unknown neighbors survive | Existing lifecycle/AppImage uninstall fixtures; no native uninstall kill test |
| 20. During native package-manager execution | process loss, signal, manager error | Native package state is authoritative and result may be unknown; absent package metadata with remaining owned payload is partial state | Inspect exact DEB/RPM package identity/version before disposition; absent metadata plus payload stops as `OutcomeUnknown`; never immediately rerun APT/DNF | Never delete dpkg/apt/rpm/dnf locks or user/server state | Quick-install interruption and partial-payload fixtures; ordinary Linux package CI is separate native lifecycle evidence |
| 21. During AppImage integration | process loss, partial launcher/record, ENOSPC | Trusted record may be complete, incomplete, missing, or unknown/newer | Inspect record and owned integration; missing record with adjacent launcher is `Incomplete`, unknown/newer fails closed | Unknown launcher and user state preserved | AppImage partial-record and removal fixtures |
| 22. During Windows Setup replacement | forced process termination after first changed owned payload copy; disk-full copy/registration | Fixture is designed to observe new target `LICENSE` bytes while the old ownership manifest and registration hash still agree; a later 64 MiB package-owned payload must remain incomplete | Start a fresh compatible Setup, revalidate the old ownership boundary, finish the target payload, and verify the final registration/manifest/payload hashes; no binary rollback | P027 ownership scope, state, disabled startup choice, and unknown files are asserted by the fixture | Focused P044 run `37610709603` installed the old fixture but found an empty `SynveilManifestSha256` value and stopped before upgrade. Current source explicitly writes the target manifest hash at `ssPostInstall` and aborts if that write fails; run `37611946019` is pending. No Windows interruption pass is claimed yet; no full runtime closure or power-loss evidence |
| 23. During obsolete owned-file cleanup | process loss or delete failure | Trusted previous and target manifests define old-minus-new set | Recompute scope from manifests; missing known obsolete files are no-op; never scan leftovers | Unknown adjacent files are never deleted | Windows ownership source contract and lifecycle fixture; interrupted native cleanup not executed |
| 24. Disk exhaustion during download/staging | ENOSPC/quota at temp create, stream, flush, fsync, promotion, directory sync | Temp may be partial; final may be absent or complete but unreported | Return finite disk-full/staging error; clean only owned temp; reverify any final file; no manager invocation | Existing destination and unrelated files preserved | Deterministic Python ENOSPC fixtures |
| 25. Disk exhaustion while writing journal state | injected ENOSPC at checkpoint boundaries | Before mutation-start: no apply. After mutation: earlier chain plus unmatched start remains | Before mutation, stop/replan; after mutation, stop and inspect; retain old checkpoints | Never delete earlier checkpoints to free space | Journal deterministic disk-full fault fixtures; not a real full filesystem |
| 26. Disk exhaustion replacing package payload | simulated adapter/native filesystem failure | Effect may be partial after durable start | Stop later effects; inspect package-owned closure and repair/reconcile | No user/server data cleanup to make room | Common-engine source/unit only; bounded real ENOSPC payload test unavailable |
| 27. Disk exhaustion during platform integration | injected or OS integration write failure | Integration record/shortcut/service may be absent or partial | Inspect owner record and platform state; repair only known-owned resources | Unknown/newer record and unknown integration files preserved | AppImage fixture; native Windows integration ENOSPC unavailable |
| 28. Process termination | forced SIGKILL after synced simulated payload mutation | Mutation-start checkpoint and payload marker survive process A | New process validates journal, reconciles marker as applied, verifies, and completes without duplicate apply | Test-owned data sentinel remains | Separate-process `process-interruption` test; not power loss |
| 29. OS reboot | reboot between any two writes | Filesystem and journal state depend on OS/filesystem flush guarantees | On next run, acquire OS lock, validate full chain/plan, inspect unmatched effect, then reconcile | No reset/downgrade/purge permitted | Native reboot unavailable; no graceful reboot is represented as a pass |
| 30. VM power loss/power-cycle | forced VM power-off at write boundary | Durable state depends on VM image, filesystem and virtual storage behavior | Reboot disposable VM, inspect journal/package/data, reconcile only from evidence | All named preservation sentinels must survive | **BLOCKED / unavailable native power-cycle evidence**; no simulated substitute claimed |

## Platform and evidence limits

Evidence classes are distinct: `source/unit`, `fixture`,
`process-interruption`, `Native CI`, `VM reboot`, and `VM power-cycle`. The
specific boundaries below name only evidence actually available for P044.

The common engine's process-interruption harness starts a second test process
after forcibly terminating the first. It validates restart reconstruction from
durable files and authoritative adapter inspection. The controlled payload is a
fixture, not a real package manager, Inno Setup, AppImage desktop environment,
or physical disk.

P044 adds a focused Windows CI fixture that compiles the repository's actual
`Synveil.iss` with pinned Inno Setup 6.7.3, then forcibly terminates Setup during
a supported upgrade and during same-version repair, then launches new Setup
processes to recover both states. The payload is synthetic and bounded
(including a 64 MiB file); the startup handoff uses the real release
`synveil-client.exe`. Earlier hosted attempts either missed the copy boundary,
failed their identity preflight, or stopped at Inno compilation because the
`64bit` registry-entry flag and `SetRegView` are unavailable in the pinned
Inno version. The fifth completed P044 attempt, run `37610709603`, compiled and
ran the old Setup but found that `SynveilManifestSha256` was empty in its HKCU
uninstall registration, so it stopped before exercising interruption. The
current source explicitly writes the target manifest hash at `ssPostInstall`
and aborts if the write fails; focused run `37611946019` is pending. This
fixture does not qualify the Qt runtime closure or establish production Windows
clean-machine behavior.
Preservation evidence uses fresh test-owned data sentinels; it does not access
or claim preservation of pre-existing user data on that runner. The current
full Windows installer run stopped before Inno because the runtime stage lacked
`MSVCP140.dll` required by `Qt6Core.dll`.

The common engine's separate-process SIGKILL fixture and deterministic
fault-injection suites are not native package-manager or physical-storage
evidence. Linux package-manager and AppImage mutation-boundary process
termination are not exercised by P044. A VM reboot, container restart, unit
test, process kill, and graceful shutdown are distinct evidence classes; none
is labeled `VM power-cycle`.

P004 `INSTALL-JOURNEY-8` keeps its `interruption-power-cycle` minimum and is not
marked native-clean-machine PASS by P044's source, fixture, or process evidence.
P045 remains the owner of the cross-platform clean-machine matrix.
