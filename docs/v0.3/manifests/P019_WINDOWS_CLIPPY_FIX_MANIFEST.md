# P019 Windows Desktop Clippy fix manifest

Status: source repair implemented; final hosted qualification is recorded in the dedicated Draft PR description. P028 remains **BLOCKED**.

## Provenance and workspace

Repository: `nghianguyen150612/Synveil`. Target: `develop/v0.3`. Branch: `fix/v03-p019-windows-desktop-clippy`. Verified base: `288222beed0c8e56a3c60dbbeb8e2835a6551750`. The original `work` checkout and its 139 staged paths were preserved; this repair uses `/workspace/Synveil-p019-fix`. P023 PR #183 remains separate, draft and unmerged at `3c496a3c75f2b0894a68905db4123f3fe35ab3a7` when inspected. No existing fix branch/PR was found. Exact final delivery SHA and completed hosted run URLs are recorded in the fix PR description and delivery report, outside the commit that they identify.

## Original failures and roots

P019 run [41205251112](https://github.com/nghianguyen150612/Synveil/actions/runs/41205251112), Windows job 124548922357, failed strict Clippy after all contract, installer, auth and network test steps passed. Ubuntu job 124548922477 passed.

- `PreviewPartiallyAvailable`: constructed only for active Linux DEB/RPM ownership evidence.
- `NativePackageManagerManaged`: constructed only for Linux package-manager artifact evidence.
- `MissingPackageManager`: emitted only by Linux package-manager inspection.
- `MissingNativeDependency`: emitted only by Linux Host dependency inspection.
- Two `NativeHostOwners::default()` calls: Windows imports the unsupported-platform unit struct; Linux imports a stateful owner with default empty session and approval state.

These internal Desktop enums have no serde derives or external wire serialization. Their Linux-only constructors and platform boundaries were audited. The four variants and their match arms now share the constructors' Linux conditional compilation. A selection helper retains the existing eligibility predicate, and Linux artifact verification still fails closed. Linux labels and package detection are preserved. The Host owner factory retains Linux `Default` initialization and directly constructs the non-Linux unit struct. It is used for initial setup and cancellation/reset. No artificial enum construction, added warning suppression, privilege/authorization change or recovery-semantic change is included. Existing reserved-state annotations are untouched. `-D warnings` remains in force.

## Validation snapshot

- Workspace rustfmt and whitespace checks passed during source repair.
- Native acceptance contract validator passed while retaining the blocked checkpoint.
- All 46 Python native-evidence negative/contract tests passed.
- Documentation validator passed all 24 groups.
- Local Rust Desktop/installer/bootstrap/host/network compilation was attempted with a temporary Rust toolchain and extracted Qt/DBus sysroot. It is blocked by missing `libsystemd.pc` and limited container disk space (127 MiB remained after compilation); no tests from that command are counted as passed. The authoritative affected-crate test and strict Clippy results must come from hosted CI. No Windows-native claim is made from Linux.
- Hosted P019 Ubuntu/Windows and global Rust CI (including Desktop Windows native Qt contracts) must pass on the final fix head. Final results and exact head are recorded in the Draft PR description; pending jobs are not passes.
- The integration base contains P020/P021/P022. P023 remains isolated in PR #183; the added Ubuntu/Windows compatibility workflow applies only the pairing-contracts diff from the exact pinned P023 head to the disposable fix CI checkout, then runs P020–P023, authentication/API/client/client-sync/network and installer regressions. Both source identities are retained in CI artifacts. No P023 commits are copied into this fix branch.

## P028 investigation

Run [41205251060](https://github.com/nghianguyen150612/Synveil/actions/runs/41205251060) built the Windows candidate (job 124548897881) and passed standard-user product checks (job 124554449456). It failed at the explicit unavailable-surface checkpoint, which emitted `p028-blockers.json` for GUI, genuine logon and installed-component named-pipe evidence. No independently reproducible product or harness defect was established. The hosted service session cannot supply the qualified physical/interactive evidence required by the existing harness.

The [native handoff](../installer/P028_WINDOWS_NATIVE_ACCEPTANCE_HANDOFF.md) records the exact environment, phases, commands, output artifacts, redaction/cleanup requirements and remaining blockers. No workflow gate or exit condition is weakened. Standard-user lifecycle tests and native Qt CI do not qualify P028.

## Remaining gates

P004-D12/D13 remain unapproved. Keep all P004, P028, P036, independent security review, actual Android/iOS pairing, real LAN/tailnet and missing authoritative roadmap gates open. No production pairing, enrollment endpoint, credentials, migration, automatic merge or P024 is introduced. A green repair removes this CI debt; P023 integration still requires its own review and explicit merge authorization and does not imply production security readiness.
