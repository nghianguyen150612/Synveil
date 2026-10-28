[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$ReleaseManifest,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSetupSha256,
    [Parameter(Mandatory=$true)][long]$ExpectedSetupSizeBytes,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedManifestSha256,
    [Parameter(Mandatory=$true)][string]$NewLogonEvidence,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$OperatorApproval
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
$checks = [ordered]@{candidate_identity='NOT_RUN';prior_real_logon='NOT_RUN';security_logoff='NOT_RUN';task_completion='NOT_RUN';client_absent_after_logoff='NOT_RUN';independent_process_check='NOT_RUN'}
$result = 'INCOMPLETE'
$errorCode = $null
$logoffTime = $null
$taskCompletionTime = $null
$targetSidHash = $null
$profileIdHash = $null
$os = $null
$setupHash = $null
$manifestHash = $null
$setupSize = $null

function Hash-P013Text([string]$Value) {
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value))).ToLowerInvariant()
}
function Convert-P013LogonId([string]$Value) {
    if ($Value -notmatch '^(?:0x)?[0-9a-fA-F]{1,16}$') { return $null }
    try { return [Convert]::ToUInt64(($Value -replace '^0x',''),16).ToString('x') } catch { return $null }
}
function Get-P013EventData([object]$Event) {
    [xml]$xml = $Event.ToXml()
    $values = @{}
    foreach ($node in $xml.SelectNodes('//*[local-name()="EventData"]/*[local-name()="Data"]')) {
        $values[[string]$node.GetAttribute('Name')] = [string]$node.InnerText
    }
    return $values
}
function Resolve-P013Sid([string]$Identity) {
    if ([string]::IsNullOrWhiteSpace($Identity)) { return $null }
    if ($Identity -match '^S-1-') { return $Identity }
    try { return ([Security.Principal.NTAccount]::new($Identity)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { return $null }
}

try {
    if ($OperatorApproval -cne $approvalText) { throw 'P013_OPERATOR_APPROVAL_REQUIRED' }
    if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') { throw 'P013_BLOCKED_BY_ENVIRONMENT' }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'P013_LOGOFF_OBSERVER_REQUIRES_SEPARATE_ADMIN_PHASE' }
    $os = Get-CimInstance Win32_OperatingSystem
    if ($os.Caption -notmatch 'Windows 11' -or $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64' -and $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        throw 'P013_LOGOFF_OBSERVER_UNSUPPORTED_ENVIRONMENT'
    }
    $setupPath = (Resolve-Path -LiteralPath $Setup).Path
    $manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest).Path
    $setupHash = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $setupSize = (Get-Item -LiteralPath $setupPath).Length
    $release = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $artifact = @($release.artifacts | Where-Object { $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe' })
    if ((Get-AuthenticodeSignature -FilePath $setupPath).Status -ne 'NotSigned' -or
        $setupHash -cne $ExpectedSetupSha256 -or $setupSize -ne $ExpectedSetupSizeBytes -or $manifestHash -cne $ExpectedManifestSha256 -or
        $release.source_commit -cne $ExpectedSourceCommit -or $artifact.Count -ne 1 -or $artifact[0].sha256 -cne $setupHash -or [long]$artifact[0].size_bytes -ne $setupSize) {
        throw 'P013_CANDIDATE_IDENTITY_MISMATCH'
    }
    $checks.candidate_identity = 'PASS'

    $priorPath = (Resolve-Path -LiteralPath $NewLogonEvidence).Path
    $prior = Get-Content -LiteralPath $priorPath -Raw | ConvertFrom-Json
    if ($prior.schema_version -ne 1 -or $prior.source_commit -cne $ExpectedSourceCommit -or
        $prior.candidate.sha256 -cne $setupHash -or [long]$prior.candidate.size_bytes -ne $setupSize -or
        $prior.candidate.release_manifest_sha256 -cne $manifestHash -or
        $prior.checks.matching_security_4624 -cne 'PASS' -or $prior.checks.interactive_logon_type -cne 'PASS' -or
        $prior.checks.exact_logon_trigger_definition -cne 'PASS' -or $prior.checks.scheduler_logon_trigger_event -cne 'PASS' -or
        $prior.checks.task_started_after_logon -cne 'PASS' -or $prior.checks.client_started_from_task -cne 'PASS' -or
        $prior.session.classification -cne 'genuine_interactive' -or
        $prior.session.user_sid_sha256 -notmatch '^[0-9a-f]{64}$' -or $prior.session.authentication_luid_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'P013_PRIOR_LOGON_EVIDENCE_INCOMPLETE'
    }
    $checks.prior_real_logon = 'PASS'
    $targetSidHash = [string]$prior.session.user_sid_sha256
    $luidHash = [string]$prior.session.authentication_luid_sha256
    $logonTime = [DateTimeOffset]::Parse([string]$prior.session.logon_time_utc).UtcDateTime
    $triggerTime = [DateTimeOffset]::Parse([string]$prior.session.task_trigger_time_utc).UtcDateTime
    $taskStartTime = [DateTimeOffset]::Parse([string]$prior.session.task_start_time_utc).UtcDateTime
    if ($taskStartTime -lt $triggerTime -or $triggerTime -lt $logonTime) { throw 'P013_PRIOR_LOGON_TIMELINE_INVALID' }

    try {
        $newLogons = @(Get-WinEvent -FilterHashtable @{LogName='Security';Id=4624;StartTime=$logonTime.AddSeconds(-10);EndTime=[DateTime]::UtcNow} -MaxEvents 2000 -ErrorAction Stop | Where-Object {
            $data = Get-P013EventData $_
            $targetLogon = Convert-P013LogonId ([string]$data.TargetLogonId)
            $targetLogon -and (Hash-P013Text $targetLogon) -ceq $luidHash -and
                (Hash-P013Text ([string]$data.TargetUserSid)) -ceq $targetSidHash -and [string]$data.LogonType -in @('2','10')
        })
        $securityEvents = @(Get-WinEvent -FilterHashtable @{LogName='Security';Id=4634;StartTime=$taskStartTime.AddSeconds(-30);EndTime=[DateTime]::UtcNow} -MaxEvents 4000 -ErrorAction Stop)
    } catch {
        $checks.security_logoff = 'BLOCKED_BY_ENVIRONMENT'
        throw 'P013_BLOCKED_BY_ENVIRONMENT: required Security event channel is unavailable to the separate observer'
    }
    if ($newLogons.Count -eq 0) { throw 'P013_PRIOR_LOGON_EVENT_NOT_RECONFIRMED' }
    $matchingLogoffs = @($securityEvents | Where-Object {
        $data = Get-P013EventData $_
        $targetLogon = Convert-P013LogonId ([string]$data.TargetLogonId)
        $targetLogon -and (Hash-P013Text $targetLogon) -ceq $luidHash -and
            (Hash-P013Text ([string]$data.TargetUserSid)) -ceq $targetSidHash -and
            $_.TimeCreated.ToUniversalTime() -ge $taskStartTime.AddSeconds(-30)
    })
    $logoff = $matchingLogoffs | Sort-Object TimeCreated | Select-Object -First 1
    if (-not $logoff) { throw 'P013_REAL_LOGOFF_EVENT_MISSING' }
    $logoffTime = $logoff.TimeCreated.ToUniversalTime()
    $checks.security_logoff = 'PASS'

    $profiles = @(Get-CimInstance Win32_UserProfile | Where-Object { Hash-P013Text ([string]$_.SID) -ceq $targetSidHash })
    if ($profiles.Count -ne 1 -or $profiles[0].Loaded -or $profiles[0].Special) { throw 'P013_LOGOFF_PROFILE_NOT_UNLOADED' }
    $ownerAccounts = @(Get-CimInstance Win32_UserAccount | Where-Object {
        $_.LocalAccount -and (Hash-P013Text ([string]$_.SID) -ceq $targetSidHash)
    })
    if ($ownerAccounts.Count -ne 1 -or $ownerAccounts[0].Name -notmatch '^synveil_p013_[0-9a-f]{8}$' -or
        $ownerAccounts[0].Description -cne 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') {
        throw 'P013_LOGOFF_USER_NOT_MARKED_DISPOSABLE'
    }
    $profileRoot = [IO.Path]::GetFullPath([string]$profiles[0].LocalPath)
    if ((Split-Path -Leaf $profileRoot) -notmatch '^synveil_p013_[0-9a-f]{8}(?:\.[0-9]+)?$') { throw 'P013_LOGOFF_PROFILE_NOT_DISPOSABLE' }
    $config = Join-Path $profileRoot 'AppData\Roaming\Synveil\client.conf'
    if (-not (Test-Path -LiteralPath $config -PathType Leaf)) { throw 'P013_LOGOFF_PROFILE_CONFIG_MISSING' }
    if ((Resolve-P013Sid (Get-Acl -LiteralPath $config).Owner) -notmatch '^S-1-' -or
        (Hash-P013Text (Resolve-P013Sid (Get-Acl -LiteralPath $config).Owner)) -cne $targetSidHash) {
        throw 'P013_LOGOFF_PROFILE_CONFIG_OWNER_MISMATCH'
    }
    $configLines = @([IO.File]::ReadAllLines($config) | Where-Object { $_ -match '^\s*profile_id\s*=' })
    if ($configLines.Count -ne 1 -or $configLines[0] -notmatch '^\s*profile_id\s*=\s*([0-9a-f-]{36})\s*$') { throw 'P013_LOGOFF_PROFILE_ID_UNAVAILABLE' }
    $profileId = $Matches[1].ToLowerInvariant()
    if ($profileId -notmatch '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') { throw 'P013_LOGOFF_PROFILE_ID_UNAVAILABLE' }
    $profileIdHash = Hash-P013Text $profileId
    $taskPath = '\Synveil\BackgroundClient\'
    $taskName = 'profile-' + $profileId
    $task = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction Stop
    $taskXml = [xml](Export-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction Stop)
    $ns = [Xml.XmlNamespaceManager]::new($taskXml.NameTable)
    $ns.AddNamespace('t','http://schemas.microsoft.com/windows/2004/02/mit/task')
    $taskPrincipal = [string]$taskXml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:UserId',$ns).InnerText
    $taskOwnerSid = Resolve-P013Sid $taskPrincipal
    $taskActions = @($task.Actions)
    $installedClient = Join-Path $profileRoot 'AppData\Local\Programs\Synveil\synveil-client.exe'
    $taskTriggers = @($taskXml.SelectNodes('/t:Task/t:Triggers/t:LogonTrigger',$ns))
    $triggerUser = if ($taskTriggers.Count -eq 1) { Resolve-P013Sid ([string]$taskTriggers[0].UserId) } else { $null }
    $taskLogonType = [string]$taskXml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:LogonType',$ns).InnerText
    $taskRunLevel = [string]$taskXml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:RunLevel',$ns).InnerText
    $taskWorkingDirectory = Join-Path $profileRoot 'AppData\Local\Programs\Synveil'
    if ($taskOwnerSid -notmatch '^S-1-' -or (Hash-P013Text $taskOwnerSid) -cne $targetSidHash -or $taskActions.Count -ne 1 -or
        [IO.Path]::GetFullPath([string]$taskActions[0].Execute) -ine [IO.Path]::GetFullPath($installedClient) -or
        [string]$taskActions[0].Arguments -or [IO.Path]::GetFullPath([string]$taskActions[0].WorkingDirectory) -ine [IO.Path]::GetFullPath($taskWorkingDirectory) -or
        $taskTriggers.Count -ne 1 -or $taskTriggers[0].Enabled -cne 'true' -or $triggerUser -ine $taskOwnerSid -or
        $taskLogonType -cne 'InteractiveToken' -or $taskRunLevel -cne 'LeastPrivilege' -or $taskXml.SelectNodes('//t:Password',$ns).Count -ne 0) {
        throw 'P013_LOGOFF_TASK_IDENTITY_MISMATCH'
    }

    try {
        $taskEvents = @(Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-TaskScheduler/Operational';Id=100,102;StartTime=$triggerTime.AddSeconds(-5);EndTime=[DateTime]::UtcNow} -MaxEvents 4000 -ErrorAction Stop)
    } catch { throw 'P013_BLOCKED_BY_ENVIRONMENT: Task Scheduler Operational channel is unavailable' }
    $matchingStarts = @($taskEvents | Where-Object {
        if ($_.Id -ne 100) { return $false }
        $data = Get-P013EventData $_
        ([string]$data.TaskName).TrimEnd('\') -ieq ($taskPath + $taskName).TrimEnd('\') -and
            $_.TimeCreated.ToUniversalTime() -ge $triggerTime -and $_.TimeCreated.ToUniversalTime() -le $triggerTime.AddSeconds(30)
    })
    if ($matchingStarts.Count -eq 0) { throw 'P013_TASK_START_EVENT_NOT_RECONFIRMED' }
    $matchingCompletions = @($taskEvents | Where-Object {
        if ($_.Id -ne 102) { return $false }
        $data = Get-P013EventData $_
        $at = $_.TimeCreated.ToUniversalTime()
        ([string]$data.TaskName).TrimEnd('\') -ieq ($taskPath + $taskName).TrimEnd('\') -and
            $at -ge $logoffTime.AddSeconds(-30) -and $at -le $logoffTime.AddSeconds(90)
    })
    $completion = $matchingCompletions | Sort-Object TimeCreated | Select-Object -First 1
    if (-not $completion) { throw 'P013_TASK_COMPLETION_AFTER_LOGOFF_MISSING' }
    $taskCompletionTime = $completion.TimeCreated.ToUniversalTime()
    $checks.task_completion = 'PASS'

    $productProcesses = @(Get-CimInstance Win32_Process -Filter "Name='synveil-client.exe' OR Name='synveil-desktop.exe'")
    $ownedProcesses = [Collections.Generic.List[int]]::new()
    foreach ($process in $productProcesses) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction Stop
        if ($owner.ReturnValue -ne 0) { throw 'P013_LOGOFF_PROCESS_OWNER_UNVERIFIABLE' }
        if ((Hash-P013Text ([string]$owner.Sid)) -ceq $targetSidHash) { $ownedProcesses.Add([int]$process.ProcessId) }
    }
    if ($ownedProcesses.Count -ne 0) { throw 'P013_CLIENT_REMAINS_AFTER_LOGOFF' }
    $checks.client_absent_after_logoff = 'PASS'; $checks.independent_process_check = 'PASS'
    $result = 'PASS'
} catch {
    $message = [string]$_.Exception.Message
    $result = if ($message -match '^P013_BLOCKED_BY_ENVIRONMENT') { 'BLOCKED_BY_ENVIRONMENT' } else { 'FAIL' }
    $errorCode = if ($message -match '^P013_[A-Z0-9_]+') { $Matches[0] } else { 'P013_LOGOFF_EVIDENCE_FAILURE' }
    if ($result -eq 'BLOCKED_BY_ENVIRONMENT') {
        foreach ($name in @('security_logoff','task_completion','client_absent_after_logoff','independent_process_check')) {
            if ($checks[$name] -eq 'NOT_RUN') { $checks[$name] = 'BLOCKED_BY_ENVIRONMENT' }
        }
    }
    if ($result -eq 'FAIL') {
        foreach ($name in @('security_logoff','task_completion','client_absent_after_logoff','independent_process_check')) {
            if ($checks[$name] -eq 'NOT_RUN') { $checks[$name] = 'FAIL' }
        }
    }
}

$harnessHash = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
$record = [ordered]@{
    schema_version=1;result=$result;execution_id='p013-logoff-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ');source_commit=$ExpectedSourceCommit
    candidate=[ordered]@{sha256=$setupHash;size_bytes=$setupSize;release_manifest_sha256=$manifestHash}
    environment=[ordered]@{edition=if($os){[string]$os.Caption}else{'unavailable'};build=if($os){[int]$os.BuildNumber}else{0};architecture='x86_64';harness_revision_sha256=$harnessHash;observer='separate_admin_phase';operator_approved=($OperatorApproval -ceq $approvalText)}
    session=[ordered]@{user_sid_sha256=$targetSidHash;authentication_luid_sha256=$luidHash;profile_id_sha256=$profileIdHash;logon_time_utc=if($logonTime){$logonTime.ToString('yyyy-MM-ddTHH:mm:ssZ')}else{$null};logoff_time_utc=if($logoffTime){$logoffTime.ToString('yyyy-MM-ddTHH:mm:ssZ')}else{$null};task_completion_time_utc=if($taskCompletionTime){$taskCompletionTime.ToString('yyyy-MM-ddTHH:mm:ssZ')}else{$null}}
    checks=$checks;logoff_shutdown=if($result -eq 'PASS'){'PASS'}elseif($result -eq 'BLOCKED_BY_ENVIRONMENT'){'BLOCKED_BY_ENVIRONMENT'}else{'FAIL'};cleanup='NOT_RUN'
    redaction=[ordered]@{raw_event_data_recorded=$false;user_names_recorded=$false;paths_recorded=$false;credentials_recorded=$false;raw_logon_ids_recorded=$false};error_code=$errorCode
}
$output = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-logoff-shutdown-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
$record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $output -Encoding utf8
Write-Host "P013 logoff shutdown evidence: $result"
Write-Host "Evidence: $output"
if ($result -eq 'PASS') { exit 0 }
if ($result -eq 'BLOCKED_BY_ENVIRONMENT') { exit 2 }
exit 1
