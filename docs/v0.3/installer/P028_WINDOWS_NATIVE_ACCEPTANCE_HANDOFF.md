# P028 Windows native acceptance handoff

Status: **P028_SOURCE_READY / NATIVE_ACCEPTANCE_BLOCKED**. This handoff does not change any mandatory workflow exit condition.

## Inspected evidence

Run [41205251060](https://github.com/nghianguyen150612/Synveil/actions/runs/41205251060), source `3c496a3c75f2b0894a68905db4123f3fe35ab3a7`, built the candidate successfully (job 124548897881). Job 124554449456 authenticated the transferred candidate and passed `Native standard-user product acceptance`. It then failed `Record required unavailable surfaces honestly`, exactly as the workflow requires. Its log records `interactive_gui=BLOCKED`, `real_logon=BLOCKED`, and `named_pipe=BLOCKED` and writes `p028-blockers.json` before throwing `BLOCKED_BY_ENVIRONMENT`.

| Classification | Finding |
|---|---|
| Software defect | None independently reproduced by this failure. Candidate build and bounded standard-user checks passed. |
| Runner capability | The GitHub-hosted Windows service session is not a qualified interactive user desktop or genuine new-logon environment. |
| Hardware/session | A dedicated interactive Windows 11 x86-64 machine or snapshotted VM with working display/input is required. No special peripheral is required by the inspected harness. |
| Missing evidence | Qualified GUI stages/options, installed desktop-to-client IPC, OS-enforced cross-user pipe denial, real logon and LogonTrigger, logoff/shutdown, uninstall preservation, and independent cleanup records. Hosted lifecycle results do not supply these records. |

The existing source harness deliberately rejects CI/hosted execution. Moving the hosted job to another service runner or removing CI environment variables does not qualify it. The workflow and its explicit failing checkpoint remain unchanged.

## Required environment

Use Windows 11 x86-64 (build 22000 or later), PowerShell 7, Python 3, Rust/MSVC and Qt prerequisites needed to build the installed-component probe. Use a local interactive console session, not session 0 or a headless runner. Take a VM snapshot first. Have two distinct marked disposable standard-user accounts and a separate administrative observer. Provision/cleanup and logoff observation use explicitly approved administrative phases; Setup, installed desktop/client, GUI/IPC and new-logon observation use the non-elevated test user. Security and Task Scheduler operational events must be readable by the appropriate observer. Unreadable events remain blocked.

Obtain the exact Setup, release manifest, and `p028-candidate.json` from a verified producer run. Independently compare SHA-256, size, source commit, manifest digest, Setup manifest and signature status. The current harness accepts the existing unsigned qualification candidate only; this is not P036 publisher trust. Never copy historical hashes into new candidate evidence.

## Commands and phase order

Run from a clean checkout of the final fix PR head. Set `$candidateRoot` and `$evidenceRoot` to operator-selected absolute paths on the disposable machine, and independently verify the candidate identity before using it below. Preserve the exact harness commit separately from the candidate's source commit.

```powershell
$harnessCommit = (git rev-parse HEAD).Trim()
$id = Get-Content "$candidateRoot/p028-candidate.json" -Raw | ConvertFrom-Json
$approval = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
$owner = 'synveil_p013_0123abcd'
$peer = 'synveil_p013_4567abcd'
$common = @{
  Setup = "$candidateRoot/SynveilSetup.exe"
  ReleaseManifest = "$candidateRoot/SYNVEIL-RELEASE-MANIFEST.json"
  ExpectedSourceCommit = $id.source_commit
  ExpectedSetupSha256 = $id.sha256
  ExpectedSetupSizeBytes = $id.size_bytes
  ExpectedManifestSha256 = $id.release_manifest_sha256
  EvidenceDirectory = $evidenceRoot
  OperatorApproval = $approval
}
```

In the approved administrator provisioning session (passwords are prompted securely):

```powershell
./scripts/provision-windows-p013-test-user.ps1 -TestUserName $owner -OperatorApproval $approval
./scripts/provision-windows-p013-test-user.ps1 -TestUserName $peer -OperatorApproval $approval
```

Sign into the owner account at the local console. Re-establish the same verified variables in each session. Run non-elevated, advance the visible Setup wizard when prompted, and select startup so the real LogonTrigger can be observed:

```powershell
./scripts/qualify-windows-native-environment.ps1 @common
./scripts/test-windows-gui-journey.ps1 @common -ExpectedHarnessCommit $harnessCommit -StartupEnabled true -DesktopShortcutEnabled true -LaunchAfterInstall true -PeerUserName $peer
[DateTimeOffset]::UtcNow.ToString('o') | Set-Content "$evidenceRoot/before-logon.txt"
```

Sign out and genuinely sign back in as the same owner. Do not manually run the task. Then run non-elevated:

```powershell
$notBeforeText = Get-Content "$evidenceRoot/before-logon.txt"
$notBefore = [DateTimeOffset]::Parse($notBeforeText)
./scripts/collect-windows-new-logon-evidence.ps1 @common -NotBeforeUtc $notBefore
```

Sign out again. In the separate approved administrative observer session:

```powershell
./scripts/collect-windows-logoff-evidence.ps1 @common -NewLogonEvidence "$evidenceRoot/windows-new-logon-v1.json"
```

Sign into the owner once more, run non-elevated uninstall/preservation checks, then sign out:

```powershell
./scripts/test-windows-installed-uninstall.ps1 @common -ExpectedHarnessCommit $harnessCommit
```

In the separate approved administrator cleanup session, with both disposable users signed out:

```powershell
./scripts/cleanup-windows-p013-test-user.ps1 @common -TestUserName $owner
$peerCommon = $common.Clone()
$peerCommon.EvidenceDirectory = "$evidenceRoot/peer-cleanup"
./scripts/cleanup-windows-p013-test-user.ps1 @peerCommon -TestUserName $peer
```

Bind and validate all records:

```powershell
python scripts/summarize-windows-native-acceptance.py --candidate-identity "$candidateRoot/p028-candidate.json" --setup "$candidateRoot/SynveilSetup.exe" --release-manifest "$candidateRoot/SYNVEIL-RELEASE-MANIFEST.json" --source-commit $id.source_commit --harness-commit $harnessCommit --evidence-directory $evidenceRoot --output "$evidenceRoot/native-acceptance-summary.json"
python scripts/validate-windows-native-evidence.py "$evidenceRoot/native-acceptance-summary.json"
```

The summarizer must exit 0 and report `PASS`; the validator must report `VALID (PASS)`. `VALID (BLOCKED_BY_ENVIRONMENT)` is a structurally valid blocker, not acceptance. Retain six bound component JSON files (`windows-native-environment-v1.json`, `windows-gui-journey-v1.json`, `windows-new-logon-v1.json`, `windows-logoff-shutdown-v1.json`, `windows-uninstall-preservation-v1.json`, `windows-p013-cleanup-v1.json`), peer cleanup, summary, candidate identity/manifests/hashes and exact producer/harness/CI references. Inspect bounded redaction and independently verify both users, profiles, tasks and processes are gone. Do not retain passwords, bearer credentials, raw pipe payloads, raw security events, or user paths in published evidence.

Until every required scenario passes, preserve P028 BLOCKED. P004 (including D12/D13), P036, independent security review, actual Android/iOS pairing, real LAN/tailnet and authoritative roadmap reconciliation remain separate gates. No P024 is started.
