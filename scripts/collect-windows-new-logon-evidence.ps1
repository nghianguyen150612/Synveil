[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$ReleaseManifest,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSetupSha256,
    [Parameter(Mandatory=$true)][long]$ExpectedSetupSizeBytes,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedManifestSha256,
    [Parameter(Mandatory=$true)][DateTimeOffset]$NotBeforeUtc,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$OperatorApproval
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
if ($OperatorApproval -cne $approvalText) { throw 'P013_OPERATOR_APPROVAL_REQUIRED' }
if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') {
    throw 'P013_BLOCKED_BY_ENVIRONMENT: hosted service sessions cannot prove real user logon'
}
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'P013_REQUIRES_POWERSHELL_7' }
if (Test-Path Env:SYNVEIL_CLIENT_CONFIG) { throw 'P013_NONCANONICAL_PROFILE_OVERRIDE' }
if ($NotBeforeUtc.Offset -ne [TimeSpan]::Zero -or $NotBeforeUtc -gt [DateTimeOffset]::UtcNow) {
    throw 'P013_LOGON_TIME_BOUND_INVALID'
}

$nativeType = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
[StructLayout(LayoutKind.Sequential)] public struct P013TokenStatistics {
  public long TokenId; public long AuthenticationId; public long ExpirationTime;
  public int TokenType; public int ImpersonationLevel; public uint DynamicCharged;
  public uint DynamicAvailable; public uint GroupCount; public uint PrivilegeCount; public long ModifiedId;
}
public static class SynveilP013LogonNative {
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool ProcessIdToSessionId(uint pid, out uint sessionId);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr buffer, int length, out int needed);
  [DllImport("user32.dll", SetLastError=true)] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll", SetLastError=true)] static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool GetUserObjectInformation(IntPtr handle, int index, StringBuilder value, int length, out int needed);
  [DllImport("wtsapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WTSQuerySessionInformation(IntPtr server, uint sessionId, int infoClass, out IntPtr buffer, out uint bytes);
  [DllImport("wtsapi32.dll")] static extern void WTSFreeMemory(IntPtr buffer);
  static byte[] Info(IntPtr token, int kind) {
    int needed; GetTokenInformation(token, kind, IntPtr.Zero, 0, out needed);
    if (needed <= 0 || needed > 65536) throw new Win32Exception();
    IntPtr buffer = Marshal.AllocHGlobal(needed);
    try { if (!GetTokenInformation(token, kind, buffer, needed, out needed)) throw new Win32Exception(); byte[] result = new byte[needed]; Marshal.Copy(buffer, result, 0, needed); return result; }
    finally { Marshal.FreeHGlobal(buffer); }
  }
  public static string[] ProcessToken(uint pid) {
    IntPtr process = OpenProcess(0x1000, false, pid);
    if (process == IntPtr.Zero) throw new Win32Exception();
    try {
      IntPtr token; if (!OpenProcessToken(process, 0x0008, out token)) throw new Win32Exception();
      try {
        byte[] user = Info(token, 1); IntPtr userBuffer = Marshal.AllocHGlobal(user.Length);
        try { Marshal.Copy(user, 0, userBuffer, user.Length); string sid = new SecurityIdentifier(Marshal.ReadIntPtr(userBuffer)).Value;
          byte[] statsBytes = Info(token, 10); IntPtr statsBuffer = Marshal.AllocHGlobal(statsBytes.Length);
          try { Marshal.Copy(statsBytes, 0, statsBuffer, statsBytes.Length); P013TokenStatistics stats = Marshal.PtrToStructure<P013TokenStatistics>(statsBuffer);
            byte[] elevation = Info(token, 20); bool elevated = BitConverter.ToInt32(elevation, 0) != 0;
            uint session; if (!ProcessIdToSessionId(pid, out session)) throw new Win32Exception();
            return new[]{sid, "0x" + unchecked((ulong)stats.AuthenticationId).ToString("x"), elevated ? "true" : "false", session.ToString()};
          } finally { Marshal.FreeHGlobal(statsBuffer); }
        } finally { Marshal.FreeHGlobal(userBuffer); }
      } finally { CloseHandle(token); }
    } finally { CloseHandle(process); }
  }
  public static bool InputDesktopIsDefault() {
    IntPtr desktop = OpenInputDesktop(0, false, 1); if (desktop == IntPtr.Zero) return false;
    try { StringBuilder name = new StringBuilder(128); int needed; return GetUserObjectInformation(desktop, 2, name, name.Capacity, out needed) && name.ToString() == "Default"; }
    finally { CloseDesktop(desktop); }
  }
  public static bool WtsSessionIsActive(uint session) {
    IntPtr buffer; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 8, out buffer, out bytes)) return false;
    try { return Marshal.ReadInt32(buffer) == 0; } finally { WTSFreeMemory(buffer); }
  }
  public static string WtsSessionUser(uint session) {
    IntPtr user; uint userBytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 5, out user, out userBytes)) return null;
    try { return Marshal.PtrToStringUni(user); } finally { WTSFreeMemory(user); }
  }
  public static string WtsSessionDomain(uint session) {
    IntPtr domain; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 7, out domain, out bytes)) return null;
    try { return Marshal.PtrToStringUni(domain); } finally { WTSFreeMemory(domain); }
  }
}
'@
if (-not ('SynveilP013LogonNative' -as [type])) { Add-Type -TypeDefinition $nativeType }

function Get-EventData($Event) {
    [xml]$xml = $Event.ToXml()
    $values = @{}
    foreach ($node in $xml.SelectNodes('//*[local-name()="EventData"]/*[local-name()="Data"]')) {
        $values[[string]$node.GetAttribute('Name')] = [string]$node.InnerText
    }
    return $values
}
function Resolve-Sid([string]$Identity) {
    if ([string]::IsNullOrWhiteSpace($Identity)) { return $null }
    if ($Identity -match '^S-1-') { return $Identity }
    try { return ([Security.Principal.NTAccount]::new($Identity)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { return $null }
}
function Normalize-LogonId([string]$Value) {
    if ($Value -notmatch '^(?:0x)?[0-9a-fA-F]{1,16}$') { return $null }
    try { return [Convert]::ToUInt64(($Value -replace '^0x',''),16).ToString('x') } catch { return $null }
}
function Hash-Text([string]$Value) {
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value))).ToLowerInvariant()
}

$setupPath = (Resolve-Path -LiteralPath $Setup).Path
$manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest).Path
$setupItem = Get-Item -LiteralPath $setupPath
$setupHash = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($setupHash -cne $ExpectedSetupSha256 -or $setupItem.Length -ne $ExpectedSetupSizeBytes -or $manifestHash -cne $ExpectedManifestSha256) {
    throw 'P013_CANDIDATE_IDENTITY_MISMATCH'
}
if ((Get-AuthenticodeSignature -FilePath $setupPath).Status -ne 'NotSigned') { throw 'P013_CANDIDATE_SIGNATURE_POLICY_MISMATCH' }
$release = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$artifact = @($release.artifacts | Where-Object { $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe' })
if ($release.schema_version -ne 1 -or $release.product -cne 'Synveil' -or $release.source_commit -cne $ExpectedSourceCommit -or
    $artifact.Count -ne 1 -or $artifact[0].sha256 -cne $setupHash -or [long]$artifact[0].size_bytes -ne $setupItem.Length) {
    throw 'P013_CANDIDATE_MANIFEST_BINDING_MISMATCH'
}
$os = Get-CimInstance Win32_OperatingSystem
$arch = (Get-CimInstance Win32_ComputerSystem).SystemType
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$expectedSid = $identity.User.Value
$markedAccounts = @(Get-CimInstance Win32_UserAccount -Filter "SID='$expectedSid'" | Where-Object { $_.LocalAccount })
$disposableTestUser = $markedAccounts.Count -eq 1 -and
    $markedAccounts[0].Name -match '^synveil_p013_[0-9a-f]{8}$' -and
    $markedAccounts[0].Description -ceq 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT'
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$isAdmin = @($identity.Groups | Where-Object { $_.Value -eq $adminSid.Value }).Count -gt 0
$elevated = [SynveilP013LogonNative]::ProcessToken([uint32]$PID)[2] -eq 'true'
$isWindows11 = $os.Caption -match 'Windows 11' -and [int]$os.BuildNumber -ge 22000
$isX64 = $env:PROCESSOR_ARCHITEW6432 -eq 'AMD64' -or $env:PROCESSOR_ARCHITECTURE -eq 'AMD64'
$interactive = [SynveilP013LogonNative]::InputDesktopIsDefault()
$currentSession = [uint32]([SynveilP013LogonNative]::ProcessToken([uint32]$PID)[3])
$activeSession = [SynveilP013LogonNative]::WtsSessionIsActive($currentSession)
$wtsUser = [SynveilP013LogonNative]::WtsSessionUser($currentSession)
$wtsDomain = [SynveilP013LogonNative]::WtsSessionDomain($currentSession)
$wtsQualified = if ([string]::IsNullOrWhiteSpace($wtsDomain)) { $wtsUser } else { "$wtsDomain\$wtsUser" }
$wtsSid = Resolve-Sid $wtsQualified
$sessionMatches = $activeSession -and $interactive -and $wtsSid -ceq $expectedSid

$configPath = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) 'Synveil\client.conf'
$profileId = $null
if (Test-Path -LiteralPath $configPath -PathType Leaf) {
    $aclOwner = (Get-Acl -LiteralPath $configPath).Owner
    if ((Resolve-Sid $aclOwner) -cne $expectedSid) { throw 'P013_PROFILE_OWNER_MISMATCH' }
    $configLines = [IO.File]::ReadAllLines($configPath)
    $profileLines = @($configLines | Where-Object { $_ -match '^\s*profile_id\s*=' })
    if ($profileLines.Count -eq 1 -and $profileLines[0] -match '^\s*profile_id\s*=\s*([0-9a-f-]{36})\s*$') { $profileId = $Matches[1].ToLowerInvariant() }
}
if ($profileId -notmatch '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') { throw 'P013_PROFILE_IDENTITY_UNAVAILABLE' }

$runtimeRoot = Join-Path (Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) 'Programs') 'Synveil'
$clientPath = Join-Path $runtimeRoot 'synveil-client.exe'
$taskName = 'profile-' + $profileId
$taskPath = '\Synveil\BackgroundClient\'
$checks = [ordered]@{
    candidate_identity = if ($isWindows11 -and $isX64) { 'PASS' } else { 'FAIL' }
    disposable_test_user = if ($disposableTestUser) { 'PASS' } else { 'FAIL' }
    standard_user_token = if (-not $isAdmin -and -not $elevated) { 'PASS' } else { 'FAIL' }
    genuine_interactive_session = if ($sessionMatches) { 'PASS' } else { 'BLOCKED_BY_ENVIRONMENT' }
    matching_security_4624 = 'NOT_RUN'; interactive_logon_type = 'NOT_RUN'
    exact_logon_trigger_definition = 'NOT_RUN'; scheduler_logon_trigger_event = 'NOT_RUN'
    task_started_after_logon = 'NOT_RUN'; client_started_from_task = 'NOT_RUN'; scheduler_manual_run_absent = 'NOT_RUN'
    no_duplicate_client = 'NOT_RUN'; client_non_elevated = 'NOT_RUN'
    logoff_shutdown = 'NOT_RUN'
}
$logonTime = $null
$triggerTime = $null
$taskStartTime = $null
$authHash = $null
$clientSidHash = $null
$classification = if (-not $isWindows11 -or -not $isX64 -or -not $disposableTestUser -or $isAdmin -or $elevated) { 'unsupported' }
    elseif (-not $sessionMatches) { 'unsupported' } else { 'genuine_interactive' }
$result = 'INCOMPLETE'
$errorCode = $null

try {
    if (-not $isWindows11 -or -not $isX64 -or -not $disposableTestUser -or $isAdmin -or $elevated) { throw 'P013_LOGON_ENVIRONMENT_POLICY_FAILURE' }
    if (-not $sessionMatches) { throw 'P013_BLOCKED_BY_ENVIRONMENT: target session is not the active interactive user session' }
    if (-not (Test-Path -LiteralPath $clientPath -PathType Leaf)) { throw 'P013_INSTALLED_CLIENT_MISSING' }
    & (Join-Path $PSScriptRoot 'test-windows-installed-runtime.ps1') -RuntimeRoot $runtimeRoot -Manifest (Join-Path $runtimeRoot 'SYNVEIL-MANIFEST.txt')
    if ($LASTEXITCODE -ne 0) { throw 'P013_INSTALLED_RUNTIME_VALIDATION_FAILURE' }

    $clientProcesses = @(Get-CimInstance Win32_Process -Filter "Name='synveil-client.exe'" | Where-Object { $_.ExecutablePath -ieq $clientPath })
    if ($clientProcesses.Count -ne 1) { $checks.no_duplicate_client = 'FAIL'; throw 'P013_CLIENT_PROCESS_MISSING_OR_DUPLICATED' }
    $clientProcess = $clientProcesses[0]
    $clientToken = [SynveilP013LogonNative]::ProcessToken([uint32]$clientProcess.ProcessId)
    $clientSid = [string]$clientToken[0]
    $clientLuid = Normalize-LogonId ([string]$clientToken[1])
    if ([string]::IsNullOrWhiteSpace($clientLuid)) { throw 'P013_CLIENT_LOGON_LUID_UNAVAILABLE' }
    $clientElevated = [string]$clientToken[2] -eq 'true'
    $clientSession = [uint32]$clientToken[3]
    $clientSidHash = Hash-Text $clientSid
    $authHash = Hash-Text $clientLuid
    $checks.client_non_elevated = if (-not $clientElevated) { 'PASS' } else { 'FAIL' }
    if ($clientSid -cne $expectedSid -or $clientSession -ne $currentSession -or $clientElevated) { throw 'P013_CLIENT_TOKEN_MISMATCH' }
    $checks.no_duplicate_client = 'PASS'
    $task = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction Stop
    $taskXml = [xml](Export-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction Stop)
    $ns = [Xml.XmlNamespaceManager]::new($taskXml.NameTable)
    $ns.AddNamespace('t','http://schemas.microsoft.com/windows/2004/02/mit/task')
    $triggers = @($taskXml.SelectNodes('/t:Task/t:Triggers/t:LogonTrigger',$ns))
    $triggerUser = if ($triggers.Count -eq 1) { [string]$triggers[0].UserId } else { '' }
    $principalUser = [string]$taskXml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:UserId',$ns).InnerText
    $taskActions = @($task.Actions)
    $actionMatches = $taskActions.Count -eq 1 -and $taskActions[0].Execute -ieq $clientPath -and
        [string]::IsNullOrEmpty([string]$taskActions[0].Arguments) -and $taskActions[0].WorkingDirectory -ieq $runtimeRoot
    $principalMatches = (Resolve-Sid $triggerUser) -ceq $expectedSid -and
        (Resolve-Sid $principalUser) -ceq $expectedSid -and
        [string]$taskXml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:LogonType',$ns).InnerText -ceq 'InteractiveToken' -and
        [string]$taskXml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:RunLevel',$ns).InnerText -ceq 'LeastPrivilege' -and
        $actionMatches -and $taskXml.SelectNodes('//t:Password',$ns).Count -eq 0
    $checks.exact_logon_trigger_definition = if ($triggers.Count -eq 1 -and $triggers[0].Enabled -ceq 'true' -and $principalMatches) { 'PASS' } else { 'FAIL' }

    try {
        $securityEvents = @(Get-WinEvent -FilterHashtable @{LogName='Security';Id=4624;StartTime=$NotBeforeUtc.UtcDateTime;EndTime=[DateTime]::UtcNow} -MaxEvents 2000 -ErrorAction Stop)
        $taskEvents = @(Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-TaskScheduler/Operational';Id=100,107,110;StartTime=$NotBeforeUtc.UtcDateTime;EndTime=[DateTime]::UtcNow} -MaxEvents 4000 -ErrorAction Stop)
    } catch {
        $checks.matching_security_4624 = 'BLOCKED_BY_ENVIRONMENT'
        $checks.scheduler_logon_trigger_event = 'BLOCKED_BY_ENVIRONMENT'
        throw 'P013_BLOCKED_BY_ENVIRONMENT: required native event channels are unavailable to this non-elevated collector'
    }
    $matchingLogons = @($securityEvents | Where-Object {
        $data = Get-EventData $_
        (Normalize-LogonId ([string]$data.TargetLogonId)) -ceq $clientLuid -and
        [string]$data.TargetUserSid -ceq $expectedSid -and [string]$data.LogonType -in @('2','10') -and
        $_.TimeCreated.ToUniversalTime() -ge $NotBeforeUtc.UtcDateTime
    })
    $logon = $matchingLogons | Sort-Object TimeCreated -Descending | Select-Object -First 1
    if ($null -eq $logon) { $checks.matching_security_4624 = 'FAIL'; throw 'P013_REAL_LOGON_EVENT_MISMATCH' }
    $logonData = Get-EventData $logon
    $logonTime = $logon.TimeCreated.ToUniversalTime()
    $checks.matching_security_4624 = 'PASS'
    $checks.interactive_logon_type = if ([string]$logonData.LogonType -in @('2','10')) { 'PASS' } else { 'FAIL' }

    $matchingTriggers = @($taskEvents | Where-Object {
        if ($_.Id -ne 107) { return $false }
        $data = Get-EventData $_
        $contextSid = Resolve-Sid ([string]$data.UserContext)
        ([string]$data.TaskName).TrimEnd('\') -ieq ($task.TaskPath + $task.TaskName).TrimEnd('\') -and
            $contextSid -ceq $expectedSid -and $_.TimeCreated.ToUniversalTime() -ge $logonTime
    })
    $trigger = $matchingTriggers | Sort-Object TimeCreated | Select-Object -First 1
    if ($null -eq $trigger) { $checks.scheduler_logon_trigger_event = 'FAIL'; throw 'P013_LOGON_TRIGGER_EVENT_MISSING' }
    $triggerTime = $trigger.TimeCreated.ToUniversalTime()
    $checks.scheduler_logon_trigger_event = 'PASS'
    $matchingStarts = @($taskEvents | Where-Object {
        if ($_.Id -ne 100) { return $false }
        $data = Get-EventData $_
        ([string]$data.TaskName).TrimEnd('\') -ieq ($task.TaskPath + $task.TaskName).TrimEnd('\') -and
            $_.TimeCreated.ToUniversalTime() -ge $triggerTime -and $_.TimeCreated.ToUniversalTime() -le $triggerTime.AddSeconds(30)
    })
    $taskStart = $matchingStarts | Sort-Object TimeCreated | Select-Object -First 1
    if ($null -eq $taskStart) { $checks.task_started_after_logon = 'FAIL'; throw 'P013_TASK_START_EVENT_MISSING' }
    $taskStartTime = $taskStart.TimeCreated.ToUniversalTime()
    $checks.task_started_after_logon = 'PASS'
    $manualRuns = @($taskEvents | Where-Object {
        if ($_.Id -ne 110) { return $false }
        $data = Get-EventData $_
        ([string]$data.TaskName).TrimEnd('\') -ieq ($task.TaskPath + $task.TaskName).TrimEnd('\') -and
            $_.TimeCreated.ToUniversalTime() -ge $logonTime -and $_.TimeCreated.ToUniversalTime() -le $taskStartTime.AddSeconds(30)
    })
    if ($manualRuns.Count -gt 0) { $checks.scheduler_manual_run_absent = 'FAIL'; throw 'P013_MANUAL_TASK_RUN_CANNOT_PROVE_LOGON_TRIGGER' }
    $checks.scheduler_manual_run_absent = 'PASS'
    $started = [Diagnostics.Process]::GetProcessById([int]$clientProcess.ProcessId)
    $started.Refresh()
    $clientStart = $started.StartTime.ToUniversalTime()
    $checks.client_started_from_task = if ($clientStart -ge $triggerTime -and $clientStart -le $taskStartTime.AddSeconds(10)) { 'PASS' } else { 'FAIL' }
    if ($checks.client_started_from_task -ne 'PASS') { throw 'P013_CLIENT_START_TIME_MISMATCH' }
    $result = 'PASS'
} catch {
    $message = [string]$_.Exception.Message
    if ($message -match '^P013_BLOCKED_BY_ENVIRONMENT') { $result = 'BLOCKED_BY_ENVIRONMENT'; $errorCode = 'P013_BLOCKED_BY_ENVIRONMENT' }
    elseif ($message -match '^P013_') { $result = 'FAIL'; $errorCode = ($message -split ':')[0] }
    else { $result = 'INCOMPLETE'; $errorCode = 'P013_LOGON_EVIDENCE_INCOMPLETE' }
}

$harnessPath = $PSCommandPath
$harnessHash = (Get-FileHash -LiteralPath $harnessPath -Algorithm SHA256).Hash.ToLowerInvariant()
$record = [ordered]@{
    schema_version=1; result=$result; execution_id='p013-logon-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ'); source_commit=$ExpectedSourceCommit
    candidate=[ordered]@{sha256=$setupHash;size_bytes=$setupItem.Length;release_manifest_sha256=$manifestHash}
    environment=[ordered]@{edition=[string]$os.Caption;build=[int]$os.BuildNumber;architecture=if($isX64){'x86_64'}else{[string]$arch};qualification=if($isWindows11 -and $isX64 -and $sessionMatches -and -not $isAdmin -and -not $elevated){'qualified_isolated_native'}else{'unsupported'};operator_approved=$true;harness_revision_sha256=$harnessHash}
    session=[ordered]@{classification=$classification;process_session_id=[uint32]$currentSession;user_sid_sha256=(Hash-Text $expectedSid);authentication_luid_sha256=$authHash;logon_time_utc=if($logonTime){$logonTime.ToString('yyyy-MM-ddTHH:mm:ssZ')}else{$null};task_trigger_time_utc=if($triggerTime){$triggerTime.ToString('yyyy-MM-ddTHH:mm:ssZ')}else{$null};task_start_time_utc=if($taskStartTime){$taskStartTime.ToString('yyyy-MM-ddTHH:mm:ssZ')}else{$null}}
    checks=$checks; logoff_shutdown='NOT_RUN'; independent_cleanup='NOT_RUN'
    redaction=[ordered]@{raw_event_data_recorded=$false;user_names_recorded=$false;paths_recorded=$false;credentials_recorded=$false}; error_code=$errorCode
}
$output = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-new-logon-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
$record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $output -Encoding utf8
Write-Host "P013 new-logon evidence: $result"
Write-Host "Evidence: $output"
if ($result -eq 'PASS') { exit 0 }
if ($result -eq 'BLOCKED_BY_ENVIRONMENT') { exit 2 }
if ($result -eq 'FAIL') { exit 1 }
exit 3
