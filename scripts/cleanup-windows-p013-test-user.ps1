[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidatePattern('^synveil_p013_[0-9a-f]{8}$')][string]$TestUserName,
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$ReleaseManifest,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSetupSha256,
    [Parameter(Mandatory=$true)][long]$ExpectedSetupSizeBytes,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedManifestSha256,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$OperatorApproval
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
$checks = [ordered]@{candidate_identity='NOT_RUN';account_removed='NOT_RUN';profile_removed='NOT_RUN';task_removed='NOT_RUN';process_absent='NOT_RUN';independent_verification='NOT_RUN'}
$result = 'INCOMPLETE'
$errorCode = $null
$sidHash = $null
$setupHash = $null
$manifestHash = $null
$setupSize = $null
$os = $null
$profilePath = Join-Path $env:SystemDrive "Users\$TestUserName"
$targetSid = $null

function Resolve-P013Sid([string]$Identity) {
    if ([string]::IsNullOrWhiteSpace($Identity)) { return $null }
    if ($Identity -match '^S-1-') { return $Identity }
    try { return ([Security.Principal.NTAccount]::new($Identity)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { return $null }
}
function Get-P013OwnedProcesses([string]$Sid) {
    $found = [Collections.Generic.List[int]]::new()
    foreach ($process in @(Get-CimInstance Win32_Process -Filter "Name='synveil-client.exe' OR Name='synveil-desktop.exe'")) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction Stop
        if ($owner.ReturnValue -ne 0) { throw 'P013_PROCESS_OWNER_UNVERIFIABLE' }
        if ([string]$owner.Sid -ceq $Sid) { $found.Add([int]$process.ProcessId) }
    }
    return @($found)
}
function Assert-P013TaskIsOwned([object]$Task, [string]$Sid, [string]$RuntimeRoot, [string]$ClientPath) {
    if ($Task.TaskPath -cne '\Synveil\BackgroundClient\' -or $Task.TaskName -notmatch '^profile-[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') {
        throw 'P013_TASK_OWNERSHIP_MISMATCH'
    }
    $xml = [xml](Export-ScheduledTask -TaskPath $Task.TaskPath -TaskName $Task.TaskName -ErrorAction Stop)
    $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
    $ns.AddNamespace('t','http://schemas.microsoft.com/windows/2004/02/mit/task')
    $triggers = @($xml.SelectNodes('/t:Task/t:Triggers/t:LogonTrigger',$ns))
    $principal = Resolve-P013Sid ([string]$xml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:UserId',$ns).InnerText)
    $triggerUser = if ($triggers.Count -eq 1) { Resolve-P013Sid ([string]$triggers[0].UserId) } else { $null }
    $logonType = [string]$xml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:LogonType',$ns).InnerText
    $runLevel = [string]$xml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:RunLevel',$ns).InnerText
    $actions = @($Task.Actions)
    $actionMatch = $actions.Count -eq 1 -and [IO.Path]::GetFullPath([string]$actions[0].Execute) -ieq [IO.Path]::GetFullPath($ClientPath) -and
        [string]::IsNullOrEmpty([string]$actions[0].Arguments) -and [IO.Path]::GetFullPath([string]$actions[0].WorkingDirectory) -ieq [IO.Path]::GetFullPath($RuntimeRoot)
    if ($triggers.Count -ne 1 -or $triggers[0].Enabled -cne 'true' -or $principal -cne $Sid -or $triggerUser -cne $Sid -or
        $logonType -cne 'InteractiveToken' -or $runLevel -cne 'LeastPrivilege' -or -not $actionMatch -or
        $xml.SelectNodes('//t:Password',$ns).Count -ne 0) { throw 'P013_TASK_OWNERSHIP_MISMATCH' }
}

try {
    if ($OperatorApproval -cne $approvalText) { throw 'P013_OPERATOR_APPROVAL_REQUIRED' }
    if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') { throw 'P013_BLOCKED_BY_ENVIRONMENT' }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'P013_CLEANUP_REQUIRES_SEPARATE_ADMIN_PHASE' }
    $os = Get-CimInstance Win32_OperatingSystem
    if ($os.Caption -notmatch 'Windows 11' -or $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64' -and $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        throw 'P013_CLEANUP_UNSUPPORTED_ENVIRONMENT'
    }
    $setupPath = (Resolve-Path -LiteralPath $Setup).Path
    $manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest).Path
    $setupHash = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $setupSize = (Get-Item -LiteralPath $setupPath).Length
    $release = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $artifact = @($release.artifacts | Where-Object { $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe' })
    if ($setupHash -cne $ExpectedSetupSha256 -or $setupSize -ne $ExpectedSetupSizeBytes -or $manifestHash -cne $ExpectedManifestSha256 -or
        $release.source_commit -cne $ExpectedSourceCommit -or $artifact.Count -ne 1 -or $artifact[0].sha256 -cne $setupHash -or [long]$artifact[0].size_bytes -ne $setupSize) {
        throw 'P013_CANDIDATE_IDENTITY_MISMATCH'
    }
    $checks.candidate_identity = 'PASS'

    $accounts = @(Get-CimInstance Win32_UserAccount -Filter "Name='$TestUserName'" | Where-Object { $_.LocalAccount })
    $profiles = @(Get-CimInstance Win32_UserProfile | Where-Object { $_.LocalPath -ieq $profilePath -or $_.LocalPath -match ('^' + [regex]::Escape((Join-Path $env:SystemDrive 'Users')) + '\\' + [regex]::Escape($TestUserName) + '(?:\.|$)') })
    if ($accounts.Count -gt 1 -or $profiles.Count -gt 1) { throw 'P013_TEST_USER_IDENTITY_AMBIGUOUS' }
    if ($accounts.Count -eq 1) {
        if ($accounts[0].Description -cne 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') { throw 'P013_TEST_USER_NOT_MARKED_DISPOSABLE' }
        $targetSid = [string]$accounts[0].SID
        $adminGroup = Get-LocalGroup -SID 'S-1-5-32-544' -ErrorAction Stop
        if (@(Get-LocalGroupMember -Group $adminGroup | Where-Object { $_.SID.Value -ceq $targetSid }).Count -gt 0) {
            throw 'P013_TEST_USER_MUST_NOT_BE_ADMINISTRATOR'
        }
    }
    if ($profiles.Count -eq 1) {
        if ($profiles[0].Special -or $profiles[0].Loaded) { throw 'P013_TEST_PROFILE_STILL_ACTIVE_OR_SPECIAL' }
        if ($targetSid -and [string]$profiles[0].SID -cne $targetSid) { throw 'P013_TEST_PROFILE_SID_MISMATCH' }
        if (-not $targetSid) { $targetSid = [string]$profiles[0].SID }
        $profilePath = [IO.Path]::GetFullPath([string]$profiles[0].LocalPath)
        $root = [IO.Path]::GetFullPath((Join-Path $env:SystemDrive 'Users')).TrimEnd('\') + '\'
        if (-not $profilePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -or
            (Split-Path -Leaf $profilePath) -notmatch '^synveil_p013_[0-9a-f]{8}(?:\.[0-9]+)?$') { throw 'P013_TEST_PROFILE_PATH_MISMATCH' }
    }
    if ($targetSid) { $sidHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($targetSid))).ToLowerInvariant() }

    if ($targetSid) {
        if (@(Get-P013OwnedProcesses $targetSid).Count -gt 0) { throw 'P013_TEST_USER_PRODUCT_PROCESS_STILL_RUNNING' }
        $checks.process_absent = 'PASS'
    } else {
        $checks.process_absent = 'PASS'
    }

    $runtimeRoot = Join-Path (Join-Path (Join-Path $profilePath 'AppData\Local') 'Programs') 'Synveil'
    $clientPath = Join-Path $runtimeRoot 'synveil-client.exe'
    $profileConfig = Join-Path $profilePath 'AppData\Roaming\Synveil\client.conf'
    $profileId = $null
    if (Test-Path -LiteralPath $profileConfig -PathType Leaf) {
        if ((Get-Item -LiteralPath $profileConfig).LinkType) { throw 'P013_PROFILE_CONFIG_REPARSE_POINT' }
        $configOwner = Resolve-P013Sid (Get-Acl -LiteralPath $profileConfig).Owner
        if (-not $targetSid -or $configOwner -cne $targetSid) { throw 'P013_PROFILE_CONFIG_OWNER_MISMATCH' }
        $profileLines = @([IO.File]::ReadAllLines($profileConfig) | Where-Object { $_ -match '^\s*profile_id\s*=' })
        if ($profileLines.Count -eq 1 -and $profileLines[0] -match '^\s*profile_id\s*=\s*([0-9a-f-]{36})\s*$') {
            $profileId = $Matches[1].ToLowerInvariant()
        }
        if ($profileId -notmatch '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') { throw 'P013_PROFILE_IDENTITY_UNAVAILABLE' }
    }
    $taskPath = '\Synveil\BackgroundClient\'
    $targetTask = $null
    if ($profileId) {
        $targetTask = Get-ScheduledTask -TaskPath $taskPath -TaskName ('profile-' + $profileId) -ErrorAction SilentlyContinue
    }
    if ($targetTask) {
        Assert-P013TaskIsOwned $targetTask $targetSid $runtimeRoot $clientPath
        Unregister-ScheduledTask -TaskPath $taskPath -TaskName $targetTask.TaskName -Confirm:$false -ErrorAction Stop
    } elseif (-not $profileId) {
        $possibleTasks = @(Get-ScheduledTask -TaskPath $taskPath -ErrorAction SilentlyContinue | Where-Object {
            $_.TaskName -match '^profile-[0-9a-f-]{36}$' -and @($_.Actions | Where-Object {
                [string]$_.Execute -ieq $clientPath
            }).Count -gt 0
        })
        if ($possibleTasks.Count -gt 0) { throw 'P013_STALE_TASK_CANNOT_BE_BOUND_TO_PROFILE' }
    }
    $checks.task_removed = 'PASS'

    if ($profiles.Count -eq 1) { Remove-CimInstance -InputObject $profiles[0] -ErrorAction Stop }
    if ($accounts.Count -eq 1) { Remove-LocalUser -Name $TestUserName -ErrorAction Stop }
    $checks.account_removed = 'PASS'
    $remainingAccount = @(Get-CimInstance Win32_UserAccount -Filter "Name='$TestUserName'" | Where-Object { $_.LocalAccount })
    $remainingProfiles = @(Get-CimInstance Win32_UserProfile | Where-Object { $_.SID -ceq $targetSid })
    $remainingTask = if ($profileId) { Get-ScheduledTask -TaskPath $taskPath -TaskName ('profile-' + $profileId) -ErrorAction SilentlyContinue } else { $null }
    if ($remainingAccount.Count -ne 0 -or $remainingProfiles.Count -ne 0 -or $remainingTask -or (Test-Path -LiteralPath $profilePath)) {
        throw 'P013_INDEPENDENT_CLEANUP_VERIFICATION_FAILURE'
    }
    if ($targetSid -and @(Get-P013OwnedProcesses $targetSid).Count -gt 0) { throw 'P013_INDEPENDENT_PROCESS_CLEANUP_VERIFICATION_FAILURE' }
    $checks.profile_removed = 'PASS'; $checks.independent_verification = 'PASS'; $result = 'PASS'
} catch {
    $result = if ($_.Exception.Message -eq 'P013_BLOCKED_BY_ENVIRONMENT') { 'BLOCKED_BY_ENVIRONMENT' } else { 'FAIL' }
    $errorCode = if ($_.Exception.Message -match '^P013_[A-Z0-9_]+') { $Matches[0] } else { 'P013_CLEANUP_FAILURE' }
    if ($result -eq 'FAIL') { $checks.independent_verification = 'FAIL' }
}

$harnessBinding = @(
    'scripts/cleanup-windows-p013-test-user.ps1:' + (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant(),
    'scripts/provision-windows-p013-test-user.ps1:' + (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'provision-windows-p013-test-user.ps1') -Algorithm SHA256).Hash.ToLowerInvariant()
) -join "`n"
$harnessHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($harnessBinding))).ToLowerInvariant()
$environmentQualified = $null -ne $os -and $os.Caption -match 'Windows 11' -and
    ($env:PROCESSOR_ARCHITEW6432 -eq 'AMD64' -or $env:PROCESSOR_ARCHITECTURE -eq 'AMD64') -and
    $OperatorApproval -ceq $approvalText -and $env:GITHUB_ACTIONS -ne 'true' -and -not $env:RUNNER_NAME -and $env:CI -ne 'true'
$record = [ordered]@{
    schema_version=1;result=$result;execution_id='p013-cleanup-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ');source_commit=$ExpectedSourceCommit
    candidate=[ordered]@{filename='SynveilSetup.exe';sha256=$setupHash;size_bytes=$setupSize;source_commit=$ExpectedSourceCommit;release_manifest_sha256=$manifestHash}
    environment=[ordered]@{edition=if($os){[string]$os.Caption}else{'unavailable'};build=if($os){[int]$os.BuildNumber}else{0};architecture='x86_64';harness_revision_sha256=$harnessHash;qualification=if($environmentQualified){'qualified_isolated_native'}else{'unsupported'};operator_approved=($OperatorApproval -ceq $approvalText)}
    checks=$checks;cleanup=[ordered]@{status=$result;independently_verified=($checks.independent_verification -ceq 'PASS')}
    redaction=[ordered]@{user_sid_sha256=$sidHash;user_names_recorded=$false;paths_recorded=$false;credentials_recorded=$false;raw_event_data_recorded=$false};error_code=$errorCode
}
$evidencePath = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-p013-cleanup-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $evidencePath) -Force | Out-Null
$record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $evidencePath -Encoding utf8
Write-Host "P013 isolated test-user cleanup: $result"
Write-Host "Evidence: $evidencePath"
if ($result -eq 'PASS') { exit 0 }
if ($result -eq 'BLOCKED_BY_ENVIRONMENT') { exit 2 }
exit 1
