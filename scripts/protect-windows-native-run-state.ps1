[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Initialize','Verify','Advance','ObserveInitialSession','ObserveLogoff','ObserveNewLogon','ObserveLogonTrigger','VerifyObserverSequence','CaptureLogonTask','VerifyRoleAccess')][string]$Action,
    [Parameter(Mandatory=$true)][string]$ApprovedRoot,
    [string]$RunId,
    [Parameter(Mandatory=$true)][ValidatePattern('^S-1-5-21-(?:[0-9]+-){3}[0-9]+$')][string]$SubjectSid,
    [Parameter(Mandatory=$true)][ValidatePattern('^S-1-5-21-(?:[0-9]+-){3}[0-9]+$')][string]$ObserverSid,
    [ValidatePattern('^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$')][string]$TaskProfileId,
    [ValidatePattern('^[0-9a-f]{64}$')][string]$CandidateClientSha256,
    [uint32]$ObserverProcessId,
    [uint32]$SubjectProcessId,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$HarnessCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$CandidateSha256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory=$true)][DateTimeOffset]$ExpiresUtc,
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [ValidateSet('PREFLIGHT','CANDIDATE_VERIFIED','ENVIRONMENT_QUALIFIED','INSTALLED','GUI_VERIFIED','IPC_VERIFIED','STARTUP_CONFIGURED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED','LIFECYCLE_VERIFIED','CLEANUP_VERIFIED','EVIDENCE_FINALIZED')][string]$Phase,
    [string]$EvidenceFile
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'P028_REQUIRES_WINDOWS_POWERSHELL_7' }
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$systemSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$subject = [Security.Principal.SecurityIdentifier]::new($SubjectSid)
if ($OperatorApproval -cne $approvalText) { throw 'P028_OPERATOR_APPROVAL_REQUIRED' }
. (Join-Path $PSScriptRoot 'windows-native-observer-support.ps1')
. (Join-Path $PSScriptRoot 'windows-native-logon-task.ps1')
$observer = [Security.Principal.SecurityIdentifier]::new($ObserverSid)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$isObserverAction = $Action -in @('ObserveInitialSession','ObserveLogoff','ObserveNewLogon','ObserveLogonTrigger','VerifyObserverSequence')
if ($observer -eq $subject) { throw 'P028_OBSERVER_SUBJECT_IDENTITY_COLLISION' }
Assert-P028LocalAccount $observer '^synveil_obs_[0-9a-f]{8}$'
if ($isObserverAction) {
    [P028ObserverAccess]::AssertRoleToken([uint32]$PID, $observer.Value, $true)
    if ($identity.User -ne $observer -or -not $principal.IsInRole([Security.Principal.SecurityIdentifier]::new('S-1-5-32-573'))) { throw 'P028_OBSERVER_EFFECTIVE_TOKEN_INVALID' }
} else {
    if ($identity.User -eq $observer -or -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'P028_TRUSTED_CONTROLLER_REQUIRES_ADMIN'
    }
    if (-not [P028ObserverAccess]::IsElevated([uint32]$PID, $identity.User.Value)) { throw 'P028_TRUSTED_CONTROLLER_NOT_ELEVATED' }
}
if ($subject -eq $identity.User -or $subject -eq $adminSid -or $subject -eq $systemSid) { throw 'P028_SUBJECT_IDENTITY_INVALID' }
Assert-P028LocalAccount $subject '^synveil_p013_[0-9a-f]{8}$'
$subjectAccount = @(Get-CimInstance Win32_UserAccount -Filter "SID='$($subject.Value)' AND LocalAccount=TRUE" -ErrorAction Stop)
if ($subjectAccount.Count -ne 1 -or $subjectAccount[0].Description -cne 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') {
    throw 'P028_SUBJECT_NOT_DISPOSABLE_LOCAL_USER'
}
if ($Action -eq 'Initialize') {
    $directAdministrators = @(Get-LocalGroupMember -SID $adminSid -ErrorAction Stop)
    if (@($directAdministrators | Where-Object { $_.SID -eq $subject -or $_.SID -eq $observer }).Count -ne 0) {
        throw 'P028_ROLE_DIRECT_ADMIN_MEMBERSHIP'
    }
}
$windows = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
$computerSystem = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
if ($windows.Caption -notmatch 'Windows 11' -or [int]$windows.BuildNumber -lt 22000 -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'P028_NATIVE_PLATFORM_UNSUPPORTED' }
if ($ExpiresUtc.Offset -ne [TimeSpan]::Zero -or $ExpiresUtc -le [DateTimeOffset]::UtcNow -or $ExpiresUtc -gt [DateTimeOffset]::UtcNow.AddDays(7)) { throw 'P028_RUN_DEADLINE_INVALID' }
if ($Action -eq 'Initialize') {
    if (-not [string]::IsNullOrWhiteSpace($RunId)) { throw 'P028_RUN_ID_MUST_BE_GENERATED' }
    $RunId = [Guid]::NewGuid().ToString('N')
} elseif ($RunId -notmatch '^[0-9a-f]{32}$') { throw 'P028_RUN_ID_INVALID' }

function Assert-NoReparsePath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    $drive = [IO.Path]::GetPathRoot($full)
    $current = $drive
    $tail = $full.Substring($drive.Length).Split([IO.Path]::DirectorySeparatorChar, [StringSplitOptions]::RemoveEmptyEntries)
    foreach ($part in $tail) {
        $current = Join-Path $current $part
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'P028_REPARSE_POINT_REJECTED' }
        }
    }
    return $full
}
function Get-ExpectedAcl([string]$Path, [bool]$Inbox, [bool]$IncludeSubject = $true, [string]$ObserverAccess = 'None') {
    $acl = Get-Acl -LiteralPath $Path
    if (-not $acl.AreAccessRulesProtected) { throw 'P028_INHERITED_ACL_REJECTED' }
    $owner = $acl.GetOwner([Security.Principal.SecurityIdentifier])
    if ($owner -ne $adminSid) { throw 'P028_STATE_OWNER_MISMATCH' }
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    $expected = @{}
    $expected[$adminSid.Value] = [Security.AccessControl.FileSystemRights]::FullControl
    $expected[$systemSid.Value] = [Security.AccessControl.FileSystemRights]::FullControl
    if ($IncludeSubject) { $expected[$subject.Value] = if ($Inbox) { [Security.AccessControl.FileSystemRights]::Modify } else { [Security.AccessControl.FileSystemRights]::ReadAndExecute } }
    if ($ObserverAccess -ne 'None') {
        $expected[$observer.Value] = Get-P028ObserverRights $ObserverAccess
    }
    # Allow rules constructed by .NET include Synchronize even when the named
    # rights constant omits it. Compare the resulting mask, not effective access.
    foreach ($key in @($expected.Keys)) {
        $expected[$key] = $expected[$key] -bor [Security.AccessControl.FileSystemRights]::Synchronize
    }
    foreach ($rule in $rules) {
        $sid = $rule.IdentityReference.Value
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or
            $rule.IsInherited -or
            $rule.InheritanceFlags -ne ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit) -or
            $rule.PropagationFlags -ne [Security.AccessControl.PropagationFlags]::None -or
            -not $expected.ContainsKey($sid) -or
            $rule.FileSystemRights -ne $expected[$sid]) {
            Write-Warning ("P028_ACL_RULE_DIAGNOSTIC rights={0} mask=0x{1:X} inheritance={2} propagation={3} type={4} inherited={5} expectedPrincipal={6}" -f $rule.FileSystemRights, [int]$rule.FileSystemRights, $rule.InheritanceFlags, $rule.PropagationFlags, $rule.AccessControlType, $rule.IsInherited, $expected.ContainsKey($sid))
            throw 'P028_UNEXPECTED_ACL_RULE'
        }
        $expected.Remove($sid)
    }
    if ($expected.Count -ne 0) { throw 'P028_REQUIRED_ACL_MISSING' }
}
function Set-RunAcl([string]$Path, [bool]$Inbox, [bool]$IncludeSubject = $true, [string]$ObserverAccess = 'None') {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner($adminSid)
    $inheritance = [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit
    $entries = @(
        [pscustomobject]@{Sid=$adminSid; Rights=[Security.AccessControl.FileSystemRights]::FullControl},
        [pscustomobject]@{Sid=$systemSid; Rights=[Security.AccessControl.FileSystemRights]::FullControl}
    )
    if ($IncludeSubject) {
        $subjectRights = if ($Inbox) { [Security.AccessControl.FileSystemRights]::Modify } else { [Security.AccessControl.FileSystemRights]::ReadAndExecute }
        $entries += [pscustomobject]@{Sid=$subject; Rights=$subjectRights}
    }
    if ($ObserverAccess -ne 'None') {
        $entries += [pscustomobject]@{Sid=$observer; Rights=(Get-P028ObserverRights $ObserverAccess)}
    }
    foreach ($entry in $entries) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($entry.Sid, $entry.Rights, $inheritance,
            [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
    Get-ExpectedAcl $Path $Inbox $IncludeSubject $ObserverAccess
}

function Get-MachineDigest {
    $value = [string](Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography' -Name MachineGuid)
    if ([string]::IsNullOrWhiteSpace($value)) { throw 'P028_MACHINE_ID_UNAVAILABLE' }
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($value))).ToLowerInvariant()
}
function Save-RunState($State) {
    $temporary = Join-Path $trustedPath ('.run-state-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($State | ConvertTo-Json -Depth 12) + "`n")
    if ($bytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $statePath, $true)
}
function Hash-P028Text([string]$Value) {
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value))).ToLowerInvariant()
}
function Normalize-P028LogonId([string]$Value) {
    if ($Value -notmatch '^(?:0x)?[0-9a-fA-F]{1,16}$') { return $null }
    try { return [Convert]::ToUInt64(($Value -replace '^0x',''),16).ToString('x') } catch { return $null }
}
function Resolve-P028Sid([string]$IdentityName) {
    if ([string]::IsNullOrWhiteSpace($IdentityName)) { return $null }
    if ($IdentityName -match '^S-1-') { return $IdentityName }
    try { return ([Security.Principal.NTAccount]::new($IdentityName)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { return $null }
}
function Get-P028Event([object]$Event, [int]$ExpectedId, [string]$Channel, [string[]]$RequiredData) {
    $providerName = switch -CaseSensitive ($Channel) {
        'Security' { 'Microsoft-Windows-Security-Auditing' }
        'Microsoft-Windows-TaskScheduler/Operational' { 'Microsoft-Windows-TaskScheduler' }
        default { throw 'P028_EVENT_CHANNEL_UNSUPPORTED' }
    }
    if ($null -eq $Event -or $Event.Id -ne $ExpectedId -or $Event.LogName -cne $Channel -or
        $Event.ProviderName -cne $providerName -or $null -eq $Event.RecordId -or [long]$Event.RecordId -le 0 -or
        $null -eq $Event.TimeCreated) { throw 'P028_EVENT_SOURCE_OR_ID_INVALID' }
    $eventXml = [string]$Event.ToXml()
    if ([string]::IsNullOrWhiteSpace($eventXml) -or $eventXml.Length -gt 65536) { throw 'P028_EVENT_XML_SIZE_INVALID' }
    try { [xml]$xml = $eventXml } catch { throw 'P028_EVENT_XML_INVALID' }
    $provider = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="Provider"]')
    $systemId = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="EventID"]')
    $recordId = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="EventRecordID"]')
    $channelNode = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="Channel"]')
    $computer = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="Computer"]')
    $timeNode = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="TimeCreated"]')
    if ($null -eq $provider -or $provider.GetAttribute('Name') -cne $providerName -or
        $null -eq $systemId -or [int]$systemId.InnerText -ne $ExpectedId -or $null -eq $recordId -or
        [long]$recordId.InnerText -ne [long]$Event.RecordId -or $null -eq $channelNode -or $channelNode.InnerText -cne $Channel -or
        $null -eq $computer -or $null -eq $timeNode) { throw 'P028_EVENT_SCHEMA_MISMATCH' }
    $knownComputers = @([Environment]::MachineName)
    if (-not [string]::IsNullOrWhiteSpace([string]$computerSystem.Domain) -and [string]$computerSystem.Domain -notmatch '^(?:WORKGROUP|LOCAL)$') {
        $knownComputers += ([Environment]::MachineName + '.' + [string]$computerSystem.Domain)
    }
    if (@($knownComputers | Where-Object { $_ -ieq [string]$computer.InnerText }).Count -ne 1) { throw 'P028_EVENT_MACHINE_MISMATCH' }
    try { $eventTime = [DateTimeOffset]::Parse([string]$timeNode.GetAttribute('SystemTime')).ToUniversalTime() }
    catch { throw 'P028_EVENT_TIME_INVALID' }
    if ([Math]::Abs(($eventTime - [DateTimeOffset]$Event.TimeCreated.ToUniversalTime()).TotalSeconds) -gt 2) { throw 'P028_EVENT_TIME_MISMATCH' }
    if ($providerName -ceq 'Microsoft-Windows-TaskScheduler') {
        $versionNode = $xml.SelectSingleNode('/*[local-name()="Event"]/*[local-name()="System"]/*[local-name()="Version"]')
        if ($provider.GetAttribute('Guid') -ine '{DE7B24EA-73C8-4A09-985D-5BDADCFA9017}' -or
            $null -eq $versionNode -or $null -eq $Event.Version -or [int]$versionNode.InnerText -ne [int]$Event.Version) {
            throw 'P028_SCHEDULER_EVENT_SCHEMA_UNSUPPORTED'
        }
    }
    $data = @{}
    foreach ($node in $xml.SelectNodes('//*[local-name()="EventData"]/*[local-name()="Data"]')) {
        $name = [string]$node.GetAttribute('Name')
        if ([string]::IsNullOrWhiteSpace($name) -or $data.ContainsKey($name)) { throw 'P028_EVENT_DATA_AMBIGUOUS' }
        $data[$name] = [string]$node.InnerText
    }
    foreach ($name in $RequiredData) {
        if (-not $data.ContainsKey($name) -or [string]::IsNullOrWhiteSpace([string]$data[$name])) { throw 'P028_EVENT_FIELD_MISSING' }
    }
    $xmlBytes = [Text.Encoding]::UTF8.GetBytes($eventXml)
    return [pscustomobject]@{
        Id=$ExpectedId; RecordId=[long]$Event.RecordId; TimeUtc=$eventTime; Data=$data
        XmlSha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($xmlBytes)).ToLowerInvariant()
        Channel=$Channel; Provider=$providerName; Version=[int]$Event.Version
    }
}
function Get-P028SubjectSession([string]$ExpectedSid, [bool]$RequireOne = $true) {
    $sessions = [Collections.Generic.List[object]]::new()
    $processes = @(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" -ErrorAction Stop)
    foreach ($process in $processes) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction Stop
        if ($owner.ReturnValue -ne 0 -or [string]::IsNullOrWhiteSpace([string]$owner.Sid)) { throw 'P028_SESSION_PROCESS_OWNER_UNVERIFIABLE' }
        if ([string]$owner.Sid -cne $ExpectedSid) { continue }
        try {
            $token = [P028ObserverNative]::ProcessToken([uint32]$process.ProcessId, $ExpectedSid)
        }
        catch { throw 'P028_SESSION_TOKEN_UNVERIFIABLE' }
        $sessionId = $token.SessionId
        if ($token.Sid -cne $ExpectedSid -or $sessionId -ne [uint32]$process.SessionId) { throw 'P028_SESSION_TOKEN_MISMATCH' }
        if ($token.Elevated -or $token.Administrator) { throw 'P028_SUBJECT_TOKEN_NOT_ORDINARY_USER' }
        if (-not [P028ObserverNative]::WtsSessionIsActive($sessionId)) { continue }
        $user = [P028ObserverNative]::WtsSessionUser($sessionId)
        $domain = [P028ObserverNative]::WtsSessionDomain($sessionId)
        $qualified = if ([string]::IsNullOrWhiteSpace($domain)) { $user } else { "$domain\$user" }
        if ((Resolve-P028Sid $qualified) -cne $ExpectedSid) { throw 'P028_WTS_SUBJECT_MISMATCH' }
        $luid = Normalize-P028LogonId $token.Luid
        if ([string]::IsNullOrWhiteSpace($luid)) { throw 'P028_SESSION_LUID_INVALID' }
        $sessions.Add([pscustomobject]@{SessionId=$sessionId; Luid=$luid; LuidSha256=(Hash-P028Text $luid); Sid=$ExpectedSid})
    }
    $unique = @($sessions | Sort-Object SessionId,Luid -Unique)
    if ($RequireOne -and $unique.Count -ne 1) { throw 'P028_SUBJECT_SESSION_MISSING_OR_AMBIGUOUS' }
    return $unique
}
function Read-P028ObserverRecord([string]$Filename, [string]$ActionName, $RunState) {
    $path = Join-Path $observerInbox $Filename
    [void](Assert-NoReparsePath $path)
    $bytes = [P028SafeEvidenceReader]::Read($path, 262144)
    try { $record = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json }
    catch { throw 'P028_PRIOR_OBSERVER_RECORD_INVALID' }
    if ($record.schema_version -ne 1 -or $record.result -cne 'OBSERVED' -or $record.mode -cne 'WINDOWS_NATIVE_OBSERVER' -or
        $record.action -cne $ActionName -or $record.run_id -cne $RunId -or
        $record.machine_guid_sha256 -cne $RunState.machine_guid_sha256 -or
        $record.boot_time_sha256 -cne $RunState.boot_time_sha256 -or
        $record.subject_sid_sha256 -cne $RunState.subject_sid_sha256 -or
        $record.controller_sid_sha256 -cne $RunState.controller_sid_sha256 -or
        $record.observer_sid_sha256 -cne $RunState.observer_sid_sha256 -or
        $record.source_commit -cne $SourceCommit -or $record.harness_commit -cne $HarnessCommit -or
        $record.candidate_sha256 -cne $CandidateSha256 -or $record.manifest_sha256 -cne $ManifestSha256) {
        throw 'P028_PRIOR_OBSERVER_CONTEXT_MISMATCH'
    }
    return [pscustomobject]@{Record=$record; Bytes=$bytes; RecordDigest=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()}
}
function New-P028ObserverRecord($RunState, [string]$ActionName, $EventRecord, $Session, [object[]]$PriorRecords) {
    $now = [DateTimeOffset]::UtcNow
    $eventTime = [DateTimeOffset]$EventRecord.TimeUtc
    $created = ([DateTimeOffset]$RunState.created_at_utc).ToUniversalTime()
    $expires = ([DateTimeOffset]$RunState.expires_at_utc).ToUniversalTime()
    if ($eventTime -lt $created -or $eventTime -gt $now -or $now -gt $expires -or $now - $eventTime -gt [TimeSpan]::FromMinutes(15)) {
        throw 'P028_OBSERVER_EVENT_OUT_OF_WINDOW'
    }
    $eventKey = "$($EventRecord.Channel)|$($EventRecord.Provider)|$($EventRecord.RecordId)"
    foreach ($prior in $PriorRecords) {
        if ("$($prior.Record.event.channel)|$($prior.Record.event.provider)|$($prior.Record.event.record_id)" -ceq $eventKey) {
            throw 'P028_OBSERVER_EVENT_REPLAY'
        }
    }
    return [ordered]@{
        schema_version=1; mode='WINDOWS_NATIVE_OBSERVER'; result='OBSERVED'; action=$ActionName
        run_id=$RunId; source_commit=$SourceCommit; harness_commit=$HarnessCommit
        candidate_sha256=$CandidateSha256; manifest_sha256=$ManifestSha256
        machine_guid_sha256=$RunState.machine_guid_sha256; boot_time_sha256=$RunState.boot_time_sha256; subject_sid_sha256=$RunState.subject_sid_sha256
        observer_sid_sha256=(Hash-P028Text $identity.User.Value); controller_sid_sha256=$RunState.controller_sid_sha256
        event=[ordered]@{channel=$EventRecord.Channel;provider=$EventRecord.Provider;event_id=$EventRecord.Id
            record_id=$EventRecord.RecordId;occurred_at_utc=$eventTime.ToString('o')
            record_sha256=$EventRecord.XmlSha256;machine_guid_sha256=$RunState.machine_guid_sha256;boot_time_sha256=$RunState.boot_time_sha256
            logon_type=if($EventRecord.Data.ContainsKey('LogonType')){[int]$EventRecord.Data.LogonType}else{$null}}
        session=[ordered]@{windows_session_id=[uint32]$Session.SessionId;authentication_luid_sha256=$Session.LuidSha256
            user_sid_sha256=$RunState.subject_sid_sha256}
        prior_record_sha256=@($PriorRecords | ForEach-Object { $_.RecordDigest })
        observed_at_utc=$now.ToString('o')
        redaction=[ordered]@{raw_event_xml_recorded=$false;raw_logon_id_recorded=$false;user_names_recorded=$false;machine_guid_recorded=$false}
    }
}
function Save-P028ObserverRecord($Record, [string]$Filename, [string]$Directory = $observerInbox) {
    $destination = Join-Path $Directory $Filename
    [void](Assert-NoReparsePath $destination)
    if (Test-Path -LiteralPath $destination) { throw 'P028_OBSERVER_RECORD_ALREADY_EXISTS' }
    $temporary = Join-Path $Directory ('.observer-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Record | ConvertTo-Json -Depth 8 -Compress) + "`n")
    if ($bytes.Length -gt 262144) { throw 'P028_OBSERVER_RECORD_TOO_LARGE' }
    try {
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
        [IO.File]::Move($temporary, $destination)
    } catch {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
        throw 'P028_OBSERVER_RECORD_COMMIT_FAILED'
    }
    [void](Assert-NoReparsePath $destination)
    [void][P028SafeEvidenceReader]::Read($destination, 262144)
}
function Get-P028SecurityEvents([int]$EventId, [DateTime]$StartTime) {
    try {
        $events = @(Get-WinEvent -FilterHashtable @{LogName='Security';Id=$EventId;StartTime=$StartTime;EndTime=[DateTime]::UtcNow} -MaxEvents 4097 -ErrorAction Stop)
    } catch {
        if ($_.FullyQualifiedErrorId -notlike 'NoMatchingEventsFound,*') { throw 'P028_OBSERVER_SECURITY_CHANNEL_UNAVAILABLE' }
        $events = @()
    }
    if ($events.Count -gt 4096) { throw 'P028_OBSERVER_EVENT_QUERY_TRUNCATED' }
    return ,$events
}
function Get-P028SecurityEventByRecordId([int]$EventId, [long]$RecordId) {
    if ($EventId -notin @(4624,4634) -or $RecordId -le 0) { throw 'P028_EVENT_RECORD_REFERENCE_INVALID' }
    # EventRecordID is numeric and bounded above, so it is safe to interpolate
    # into this XPath. Re-read the OS event log instead of trusting persisted
    # observer JSON or its digest as an authority.
    $filter = "*[System[EventRecordID=$RecordId]]"
    try { $events = @(Get-WinEvent -LogName 'Security' -FilterXPath $filter -MaxEvents 2 -ErrorAction Stop) }
    catch { throw 'P028_OBSERVER_EVENT_REQUERY_FAILED' }
    if ($events.Count -ne 1 -or [long]$events[0].RecordId -ne $RecordId) { throw 'P028_OBSERVER_EVENT_REQUERY_AMBIGUOUS' }
    $required = if ($EventId -eq 4624) { @('TargetUserSid','TargetLogonId','LogonType') } else { @('TargetUserSid','TargetLogonId') }
    return Get-P028Event $events[0] $EventId 'Security' $required
}
function Assert-P028ObserverEventRequery($Record, [int]$ExpectedId, [string]$ExpectedLuidHash, [string]$ExpectedSid) {
    if ([int]$Record.event.event_id -ne $ExpectedId -or
        [long]$Record.event.record_id -le 0 -or
        [string]$Record.event.record_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'P028_OBSERVER_EVENT_REFERENCE_INVALID'
    }
    $actual = Get-P028SecurityEventByRecordId $ExpectedId ([long]$Record.event.record_id)
    $storedTime = ([DateTimeOffset]$Record.event.occurred_at_utc).ToUniversalTime()
    $actualLuid = Normalize-P028LogonId ([string]$actual.Data.TargetLogonId)
    if ([string]::IsNullOrWhiteSpace($actualLuid)) { throw 'P028_OBSERVER_EVENT_LOGON_ID_INVALID' }
    if ($actual.XmlSha256 -cne [string]$Record.event.record_sha256 -or
        $actual.TimeUtc -ne $storedTime -or
        [string]$actual.Data.TargetUserSid -cne $ExpectedSid -or
        (Hash-P028Text $actualLuid) -cne $ExpectedLuidHash) {
        throw 'P028_OBSERVER_EVENT_REQUERY_MISMATCH'
    }
    if ($ExpectedId -eq 4624 -and ([string]$actual.Data.LogonType -notin @('2','10') -or
        [int]$Record.event.logon_type -ne [int]$actual.Data.LogonType)) {
        throw 'P028_OBSERVER_EVENT_LOGON_TYPE_MISMATCH'
    }
}
function Invoke-P028ObserverAction($RunState, [string]$RequestedAction) {
    $subjectSid = $subject.Value
    $initialPath = 'P028-INITIAL-SESSION-v1.json'
    $logoffPath = 'P028-LOGOFF-v1.json'
    $prior = [Collections.Generic.List[object]]::new()
    $initial = $null
    $logoff = $null
    if ($RequestedAction -in @('ObserveLogoff','ObserveNewLogon')) {
        $initial = Read-P028ObserverRecord $initialPath 'ObserveInitialSession' $RunState
        $prior.Add([pscustomobject]@{Record=$initial.Record;RecordDigest=$initial.RecordDigest})
    }
    if ($RequestedAction -eq 'ObserveNewLogon') {
        $logoff = Read-P028ObserverRecord $logoffPath 'ObserveLogoff' $RunState
        $prior.Add([pscustomobject]@{Record=$logoff.Record;RecordDigest=$logoff.RecordDigest})
    }
    if ($RequestedAction -eq 'ObserveInitialSession') {
        $active = @(Get-P028SubjectSession $subjectSid)
        $session = $active[0]
        $events = Get-P028SecurityEvents 4624 ([DateTime](([DateTimeOffset]$RunState.created_at_utc).UtcDateTime))
        $matches = [Collections.Generic.List[object]]::new()
        foreach ($item in $events) {
            $parsed = Get-P028Event $item 4624 'Security' @('TargetUserSid','TargetLogonId','LogonType')
            $eventSid = [string]$parsed.Data.TargetUserSid
            $eventLuid = Normalize-P028LogonId ([string]$parsed.Data.TargetLogonId)
            if ($eventSid -ceq $subjectSid -and $eventLuid -and (Hash-P028Text $eventLuid) -ceq $session.LuidSha256 -and
                [string]$parsed.Data.LogonType -in @('2','10')) { $matches.Add($parsed) }
        }
        if ($matches.Count -ne 1) { throw 'P028_INITIAL_SESSION_EVENT_MISSING_OR_AMBIGUOUS' }
        $record = New-P028ObserverRecord $RunState $RequestedAction $matches[0] $session @()
        Save-P028ObserverRecord $record $initialPath
        Write-Output 'P028_OBSERVER_RECORD_RECORDED ObserveInitialSession'
        return
    }
    if ($RequestedAction -eq 'ObserveLogoff') {
        $initialRecord = $initial.Record
        $initialEventTime = ([DateTimeOffset]$initialRecord.event.occurred_at_utc).UtcDateTime
        $initialSession = [pscustomobject]@{SessionId=[uint32]$initialRecord.session.windows_session_id
            LuidSha256=[string]$initialRecord.session.authentication_luid_sha256}
        $events = Get-P028SecurityEvents 4634 $initialEventTime
        $matches = [Collections.Generic.List[object]]::new()
        foreach ($item in $events) {
            $parsed = Get-P028Event $item 4634 'Security' @('TargetUserSid','TargetLogonId')
            $eventSid = [string]$parsed.Data.TargetUserSid
            $eventLuid = Normalize-P028LogonId ([string]$parsed.Data.TargetLogonId)
            if ($eventSid -ceq $subjectSid -and $eventLuid -and (Hash-P028Text $eventLuid) -ceq $initialSession.LuidSha256 -and
                $parsed.TimeUtc -gt $initialEventTime) { $matches.Add($parsed) }
        }
        if ($matches.Count -ne 1) { throw 'P028_LOGOFF_EVENT_MISSING_OR_AMBIGUOUS' }
        if ([P028ObserverNative]::WtsSessionIsActive($initialSession.SessionId)) { throw 'P028_INITIAL_WINDOWS_SESSION_STILL_ACTIVE' }
        $record = New-P028ObserverRecord $RunState $RequestedAction $matches[0] $initialSession @($initial)
        Save-P028ObserverRecord $record $logoffPath
        Write-Output 'P028_OBSERVER_RECORD_RECORDED ObserveLogoff'
        return
    }
    if ($RequestedAction -eq 'ObserveNewLogon') {
        $initialRecord = $initial.Record
        $logoffRecord = $logoff.Record
        $initialLuidHash = [string]$initialRecord.session.authentication_luid_sha256
        $logoffTime = ([DateTimeOffset]$logoffRecord.event.occurred_at_utc).UtcDateTime
        $active = @(Get-P028SubjectSession $subjectSid)
        $session = $active[0]
        if ($session.LuidSha256 -ceq $initialLuidHash) { throw 'P028_AUTHENTICATION_SESSION_REUSED' }
        $events = Get-P028SecurityEvents 4624 $logoffTime
        $matches = [Collections.Generic.List[object]]::new()
        foreach ($item in $events) {
            $parsed = Get-P028Event $item 4624 'Security' @('TargetUserSid','TargetLogonId','LogonType')
            $eventSid = [string]$parsed.Data.TargetUserSid
            $eventLuid = Normalize-P028LogonId ([string]$parsed.Data.TargetLogonId)
            if ($eventSid -ceq $subjectSid -and $eventLuid -and (Hash-P028Text $eventLuid) -ceq $session.LuidSha256 -and
                [string]$parsed.Data.LogonType -in @('2','10') -and $parsed.TimeUtc -gt $logoffTime) { $matches.Add($parsed) }
        }
        if ($matches.Count -ne 1) { throw 'P028_NEW_LOGON_EVENT_MISSING_OR_AMBIGUOUS' }
        $record = New-P028ObserverRecord $RunState $RequestedAction $matches[0] $session @($initial,$logoff)
        Save-P028ObserverRecord $record 'P028-NEW-LOGON-v1.json'
        Write-Output 'P028_OBSERVER_RECORD_RECORDED ObserveNewLogon'
        return
    }
    throw 'P028_OBSERVER_ACTION_UNSUPPORTED'
}
function Verify-P028ObserverSequence($RunState) {
    $initial = Read-P028ObserverRecord 'P028-INITIAL-SESSION-v1.json' 'ObserveInitialSession' $RunState
    $logoff = Read-P028ObserverRecord 'P028-LOGOFF-v1.json' 'ObserveLogoff' $RunState
    $newLogon = Read-P028ObserverRecord 'P028-NEW-LOGON-v1.json' 'ObserveNewLogon' $RunState
    $subjectSid = $subject.Value
    Assert-P028ObserverEventRequery $initial.Record 4624 ([string]$initial.Record.session.authentication_luid_sha256) $subjectSid
    Assert-P028ObserverEventRequery $logoff.Record 4634 ([string]$initial.Record.session.authentication_luid_sha256) $subjectSid
    Assert-P028ObserverEventRequery $newLogon.Record 4624 ([string]$newLogon.Record.session.authentication_luid_sha256) $subjectSid
    $records = @($initial.Record,$logoff.Record,$newLogon.Record)
    $expectedObserver = Hash-P028Text $identity.User.Value
    $created = ([DateTimeOffset]$RunState.created_at_utc)
    $deadline = ([DateTimeOffset]$RunState.expires_at_utc)
    foreach ($record in $records) {
        if ($record.observer_sid_sha256 -cne $expectedObserver -or
            $record.event.channel -cne 'Security' -or $record.event.provider -cne 'Microsoft-Windows-Security-Auditing' -or
            [long]$record.event.record_id -le 0 -or $record.event.record_sha256 -notmatch '^[0-9a-f]{64}$' -or
            [string]$record.event.machine_guid_sha256 -cne $RunState.machine_guid_sha256 -or
            [string]$record.boot_time_sha256 -cne $RunState.boot_time_sha256 -or
            [string]$record.session.user_sid_sha256 -cne $RunState.subject_sid_sha256 -or
            [int]$record.session.windows_session_id -le 0 -or
            [string]$record.session.authentication_luid_sha256 -notmatch '^[0-9a-f]{64}$') {
            throw 'P028_OBSERVER_SEQUENCE_BINDING_INVALID'
        }
        $eventTime = ([DateTimeOffset]$record.event.occurred_at_utc).ToUniversalTime()
        $observedTime = ([DateTimeOffset]$record.observed_at_utc).ToUniversalTime()
        if ($eventTime -lt $created -or $eventTime -gt $observedTime -or $observedTime -gt $deadline -or
            $observedTime -gt [DateTimeOffset]::UtcNow -or
            [DateTimeOffset]::UtcNow - $observedTime -gt [TimeSpan]::FromMinutes(15)) {
            throw 'P028_OBSERVER_SEQUENCE_TIME_INVALID'
        }
    }
    if ([int]$initial.Record.event.event_id -ne 4624 -or [int]$logoff.Record.event.event_id -ne 4634 -or
        [int]$newLogon.Record.event.event_id -ne 4624 -or
        [string]$logoff.Record.session.authentication_luid_sha256 -cne [string]$initial.Record.session.authentication_luid_sha256 -or
        [int]$logoff.Record.session.windows_session_id -ne [int]$initial.Record.session.windows_session_id -or
        [string]$newLogon.Record.session.authentication_luid_sha256 -ceq [string]$initial.Record.session.authentication_luid_sha256) {
        throw 'P028_OBSERVER_SEQUENCE_SESSION_MISMATCH'
    }
    $initialTime = ([DateTimeOffset]$initial.Record.event.occurred_at_utc).ToUniversalTime()
    $logoffTime = ([DateTimeOffset]$logoff.Record.event.occurred_at_utc).ToUniversalTime()
    $newLogonTime = ([DateTimeOffset]$newLogon.Record.event.occurred_at_utc).ToUniversalTime()
    if ($initialTime -ge $logoffTime -or $logoffTime -ge $newLogonTime) { throw 'P028_OBSERVER_SEQUENCE_ORDER_INVALID' }
    $newReferences = @($newLogon.Record.prior_record_sha256)
    if ($newReferences.Count -ne 2 -or $newReferences[0] -cne $initial.RecordDigest -or $newReferences[1] -cne $logoff.RecordDigest) {
        throw 'P028_OBSERVER_SEQUENCE_REPLAY_OR_CHAIN_INVALID'
    }
    $eventKeys = @($records | ForEach-Object { "$($_.event.channel)|$($_.event.provider)|$($_.event.record_id)" })
    if (@($eventKeys | Select-Object -Unique).Count -ne $eventKeys.Count) { throw 'P028_OBSERVER_SEQUENCE_EVENT_REPLAY' }
    if ([long]$initial.Record.event.record_id -ge [long]$logoff.Record.event.record_id -or
        [long]$logoff.Record.event.record_id -ge [long]$newLogon.Record.event.record_id -or
        @($initial.Record.prior_record_sha256).Count -ne 0 -or
        @($logoff.Record.prior_record_sha256).Count -ne 1 -or
        $logoff.Record.prior_record_sha256[0] -cne $initial.RecordDigest) {
        throw 'P028_OBSERVER_SEQUENCE_REPLAY_OR_CHAIN_INVALID'
    }
    return $newLogon
}
$allowedRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)) 'Synveil\P028'
$root = Assert-P028CanonicalPath $ApprovedRoot
if ($root -ine $allowedRoot -and -not $root.StartsWith($allowedRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'P028_APPROVED_ROOT_INVALID'
}
[void](Assert-NoReparsePath $root)
$volume = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($root))
if ($volume.DriveType -ne [IO.DriveType]::Fixed -or $volume.DriveFormat -cne 'NTFS') { throw 'P028_STATE_REQUIRES_LOCAL_NTFS' }
if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw 'P028_APPROVED_ROOT_MISSING' }
Get-ExpectedAcl $root $false $false Read
$runRoot = Join-Path $root $RunId
$trustedPath = Join-Path $runRoot 'trusted'
$inboxPath = Join-Path $runRoot 'subject-inbox'
$observerInbox = Join-Path $trustedPath 'observer-inbox'
$evidenceRoot = Join-Path $trustedPath 'evidence'
$statePath = Join-Path $trustedPath 'run-state.json'

if ($Action -eq 'Initialize') {
    if (Test-Path -LiteralPath $runRoot) { throw 'P028_RUN_DIRECTORY_EXISTS' }
    [void][IO.Directory]::CreateDirectory($runRoot)
    Set-RunAcl $runRoot $false $true Read
    [void][IO.Directory]::CreateDirectory($trustedPath)
    Set-RunAcl $trustedPath $false $false Read
    [void][IO.Directory]::CreateDirectory($inboxPath)
    Set-RunAcl $inboxPath $true
    [void][IO.Directory]::CreateDirectory($observerInbox)
    Set-RunAcl $observerInbox $false $false Write
    [void][IO.Directory]::CreateDirectory($evidenceRoot)
    Set-RunAcl $evidenceRoot $false $false
    $template = [ordered]@{
        schema_version=1; run_id=$RunId; status='ACTIVE'; mode='WINDOWS_PROTECTED_PENDING_NATIVE_AUTHORITY'
        source_commit=$SourceCommit; harness_commit=$HarnessCommit; candidate_sha256=$CandidateSha256
        manifest_sha256=$ManifestSha256; subject_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($subject.Value))).ToLowerInvariant()
        controller_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
        observer_sid_sha256=(Hash-P028Text $observer.Value)
        machine_guid_sha256=(Get-MachineDigest); boot_time_sha256=(Hash-P028Text ($windows.LastBootUpTime.ToUniversalTime().ToString('o')))
        os_build=[int]$windows.BuildNumber; architecture='x86_64'; created_at_utc=[DateTimeOffset]::UtcNow.ToString('o')
        expires_at_utc=$ExpiresUtc.ToString('o'); phases=@(); finalization_digest=$null
    }
    $json = $template | ConvertTo-Json -Depth 8
    $temporary = Join-Path $trustedPath ('.run-state-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    if ($bytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $statePath)
    Get-ExpectedAcl $runRoot $false $true Read
    Get-ExpectedAcl $trustedPath $false $false Read
    Get-ExpectedAcl $inboxPath $true
    Get-ExpectedAcl $observerInbox $false $false Write
    Get-ExpectedAcl $evidenceRoot $false $false
    Write-Output "P028_PROTECTED_RUN_CREATED $RunId"
    exit 0
}

if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) { throw 'P028_RUN_STATE_MISSING' }
[void](Assert-NoReparsePath $runRoot)
[void](Assert-NoReparsePath $trustedPath)
[void](Assert-NoReparsePath $observerInbox)
Get-ExpectedAcl $runRoot $false $true Read
Get-ExpectedAcl $trustedPath $false $false Read
Get-ExpectedAcl $observerInbox $false $false Write
$mutex = $null
if (-not $isObserverAction) {
    [void](Assert-NoReparsePath $inboxPath)
    [void](Assert-NoReparsePath $evidenceRoot)
    Get-ExpectedAcl $inboxPath $true
    Get-ExpectedAcl $evidenceRoot $false $false
    $mutex = [Threading.Mutex]::new($false, 'Global\SynveilP028-' + $RunId)
}
$hasLock = $false
try {
    if ($null -ne $mutex) {
        try { $hasLock = $mutex.WaitOne([TimeSpan]::FromSeconds(10)) } catch [Threading.AbandonedMutexException] { $hasLock = $true }
        if (-not $hasLock) { throw 'P028_COORDINATOR_LOCK_TIMEOUT' }
    }
    [void](Assert-NoReparsePath $statePath)
    $stateBytes = [P028SafeEvidenceReader]::Read($statePath, 65536)
    if ($stateBytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $state = [Text.Encoding]::UTF8.GetString($stateBytes) | ConvertFrom-Json
    if ($state.schema_version -ne 1 -or $state.run_id -cne $RunId -or @('ACTIVE','FINALIZED') -cnotcontains $state.status -or
        $state.source_commit -cne $SourceCommit -or $state.harness_commit -cne $HarnessCommit -or
        $state.candidate_sha256 -cne $CandidateSha256 -or $state.manifest_sha256 -cne $ManifestSha256) {
        throw 'P028_RUN_STATE_IDENTITY_MISMATCH'
    }
    $subjectHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($subject.Value))).ToLowerInvariant()
    $controllerHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
    if ($state.subject_sid_sha256 -cne $subjectHash) { throw 'P028_SUBJECT_IDENTITY_MISMATCH' }
    if ($state.observer_sid_sha256 -cne (Hash-P028Text $observer.Value) -or
        $state.controller_sid_sha256 -notmatch '^[0-9a-f]{64}$' -or
        $state.observer_sid_sha256 -ceq $state.controller_sid_sha256) { throw 'P028_OBSERVER_ROLE_BINDING_INVALID' }
    if (-not $isObserverAction -and $state.controller_sid_sha256 -cne $controllerHash) { throw 'P028_CONTROLLER_IDENTITY_MISMATCH' }
    if ($ExpiresUtc -ne ([DateTimeOffset]$state.expires_at_utc)) { throw 'P028_RUN_DEADLINE_MISMATCH' }
    if ([DateTimeOffset]::UtcNow -gt ([DateTimeOffset]$state.expires_at_utc)) { throw 'P028_RUN_EXPIRED' }
    $currentWindows = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    if ((Get-MachineDigest) -cne $state.machine_guid_sha256 -or [int]$currentWindows.BuildNumber -ne [int]$state.os_build) { throw 'P028_MACHINE_IDENTITY_MISMATCH' }
    if ($state.boot_time_sha256 -notmatch '^[0-9a-f]{64}$' -or
        (Hash-P028Text ($currentWindows.LastBootUpTime.ToUniversalTime().ToString('o'))) -cne $state.boot_time_sha256) { throw 'P028_MACHINE_BOOT_IDENTITY_MISMATCH' }
    # No authenticated native observer can Advance in this checkpoint. A restored
    # or tampered file claiming recorded phases/finalization must fail even Verify.
    if ($state.mode -cne 'WINDOWS_PROTECTED_PENDING_NATIVE_AUTHORITY' -or
        $state.status -cne 'ACTIVE' -or
        -not ($state.PSObject.Properties.Name -ccontains 'phases') -or
        -not ($state.phases -is [array]) -or
        $state.phases.Count -ne 0 -or
        -not ($state.PSObject.Properties.Name -ccontains 'finalization_digest') -or
        $null -ne $state.finalization_digest) { throw 'P028_UNAUTHORIZED_STATE_ADVANCEMENT' }
    if ($Action -eq 'Verify') { Write-Output "P028_PROTECTED_RUN_VERIFIED $RunId $($state.status)"; exit 0 }
    if ($state.status -cne 'ACTIVE') { throw 'P028_RUN_TERMINAL' }
    # These read-only observer actions query local OS facilities directly and
    # write OBSERVED records only. They cannot advance coordinator phases.
    if ($Action -eq 'VerifyRoleAccess') {
        Assert-P028RoleAccess
        Write-Output 'P028_ROLE_ACCESS_DIAGNOSTIC_PASS'
        exit 0
    }
    if ($Action -eq 'CaptureLogonTask') {
        try { Save-P028LogonTaskBaseline $state }
        catch {
            $reason = $_.Exception.Message
            if ($reason -cnotmatch '^P028_[A-Z0-9_]+$') { $reason = 'P028_TASK_BASELINE_NATIVE_QUERY_UNAVAILABLE' }
            throw $reason
        }
        Write-Output 'P028_LOGON_TASK_BASELINE_RECORDED'
        exit 0
    }
    if ($Action -in @('ObserveLogonTrigger','VerifyObserverSequence')) {
        try {
            $newLogon = Verify-P028ObserverSequence $state
            Invoke-P028LogonTaskObservation $state $newLogon ($Action -eq 'VerifyObserverSequence')
        } catch {
            $reason = $_.Exception.Message
            if ($reason -cnotmatch '^P028_[A-Z0-9_]+$') { $reason = 'P028_LOGON_TASK_NATIVE_QUERY_UNAVAILABLE' }
            Write-Output "P028_BLOCKED_LOGON_TRIGGER_PROVENANCE_UNAVAILABLE $reason"
            exit 2
        }
        Write-Output 'P028_BLOCKED_OBSERVER_SEQUENCE_LOGON_TRIGGER_UNSUPPORTED P028_TASK_CAUSALITY_AND_LOADED_IMAGE_UNQUALIFIED'
        exit 2
    }
    if ($Action -in @('ObserveInitialSession','ObserveLogoff','ObserveNewLogon')) {
        Invoke-P028ObserverAction $state $Action
        exit 0
    }
    # JSON fields and the directory ACL do not yet authenticate native phase
    # advancement. Do not record a native PASS until the whole authority path is qualified.
    throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'
    if ($Action -ne 'Advance' -or [string]::IsNullOrWhiteSpace($Phase) -or [string]::IsNullOrWhiteSpace($EvidenceFile)) { throw 'P028_TRANSITION_ARGUMENTS_INVALID' }
    if ($state.phases.Count -ge 13 -or $Phase -cne @('PREFLIGHT','CANDIDATE_VERIFIED','ENVIRONMENT_QUALIFIED','INSTALLED','GUI_VERIFIED','IPC_VERIFIED','STARTUP_CONFIGURED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED','LIFECYCLE_VERIFIED','CLEANUP_VERIFIED','EVIDENCE_FINALIZED')[$state.phases.Count]) { throw 'P028_PHASE_ORDER_INVALID' }
    $observerPhases = @('PREFLIGHT','CANDIDATE_VERIFIED','ENVIRONMENT_QUALIFIED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED','CLEANUP_VERIFIED','EVIDENCE_FINALIZED')
    $sourceRoot = if ($observerPhases -ccontains $Phase) { $observerInbox } else { $inboxPath }
    $sourcePath = Assert-NoReparsePath $EvidenceFile
    if ([IO.Path]::GetDirectoryName($sourcePath) -cne [IO.Path]::GetFullPath($sourceRoot) -or [IO.Path]::GetFileName($sourcePath) -cne ($Phase + '.json')) { throw 'P028_EVIDENCE_SOURCE_INVALID' }
    $raw = [P028SafeEvidenceReader]::Read($sourcePath, 4194304)
    $record = [Text.Encoding]::UTF8.GetString($raw) | ConvertFrom-Json
    $expectedSubjectHash = $state.subject_sid_sha256
    $required = @('schema_version','run_id','phase','result','source_commit','harness_commit','candidate_sha256','manifest_sha256','machine_guid_sha256','subject_sid_sha256','observer_sid_sha256','observed_at_utc','session_id','logon_identity_sha256','manual_task_run_present')
    foreach ($key in $required) { if (-not ($record.PSObject.Properties.Name -ccontains $key)) { throw 'P028_EVIDENCE_FIELD_MISSING' } }
    if ($record.schema_version -ne 1 -or $record.run_id -cne $RunId -or $record.phase -cne $Phase -or $record.result -cne 'PASS' -or
        $record.source_commit -cne $SourceCommit -or $record.harness_commit -cne $HarnessCommit -or
        $record.candidate_sha256 -cne $CandidateSha256 -or $record.manifest_sha256 -cne $ManifestSha256 -or
        $record.machine_guid_sha256 -cne $state.machine_guid_sha256 -or $record.subject_sid_sha256 -cne $expectedSubjectHash) { throw 'P028_EVIDENCE_CONTEXT_MISMATCH' }
    $observedAt = ([DateTimeOffset]$record.observed_at_utc)
    if ($observedAt.Offset -ne [TimeSpan]::Zero -or $observedAt -lt ([DateTimeOffset]$state.created_at_utc) -or
        $observedAt -gt [DateTimeOffset]::UtcNow.AddMinutes(2) -or $observedAt -gt ([DateTimeOffset]$state.expires_at_utc)) { throw 'P028_EVIDENCE_TIME_INVALID' }
    if ($state.phases.Count -gt 0 -and $observedAt -lt [DateTimeOffset]::Parse($state.phases[-1].observed_at_utc)) { throw 'P028_EVIDENCE_TIME_ORDER_INVALID' }
    if ($observerPhases -ccontains $Phase) {
        $observerHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
        if ($record.observer_sid_sha256 -cne $observerHash) { throw 'P028_OBSERVER_IDENTITY_MISMATCH' }
    }
    if (@('GUI_VERIFIED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED') -ccontains $Phase) {
        if ([int]$record.session_id -le 0 -or $record.logon_identity_sha256 -notmatch '^[0-9a-f]{64}$') { throw 'P028_LOGON_BINDING_INVALID' }
    }
    if (@('LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED') -ccontains $Phase) {
        $guiEvidencePath = Join-Path $evidenceRoot 'GUI_VERIFIED.json'
        if (-not (Test-Path -LiteralPath $guiEvidencePath -PathType Leaf)) { throw 'P028_INITIAL_SESSION_EVIDENCE_MISSING' }
        $initialSession = [Text.Encoding]::UTF8.GetString([P028SafeEvidenceReader]::Read($guiEvidencePath, 4194304)) | ConvertFrom-Json
        if ($Phase -eq 'LOGOFF_OBSERVED' -and ([int]$record.session_id -ne [int]$initialSession.session_id -or $record.logon_identity_sha256 -cne $initialSession.logon_identity_sha256)) { throw 'P028_LOGOFF_IDENTITY_MISMATCH' }
        if ($Phase -in @('NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED')) {
            $newEvidencePath = Join-Path $evidenceRoot 'NEW_LOGON_OBSERVED.json'
            if ($Phase -eq 'LOGON_TRIGGER_VERIFIED' -and -not (Test-Path -LiteralPath $newEvidencePath -PathType Leaf)) { throw 'P028_NEW_LOGON_EVIDENCE_MISSING' }
            $newSession = if ($Phase -eq 'LOGON_TRIGGER_VERIFIED') { [Text.Encoding]::UTF8.GetString([P028SafeEvidenceReader]::Read($newEvidencePath, 4194304)) | ConvertFrom-Json } else { $record }
            if ($newSession.logon_identity_sha256 -ceq $initialSession.logon_identity_sha256) { throw 'P028_LOGON_REPLAY' }
            if ($Phase -eq 'LOGON_TRIGGER_VERIFIED' -and ([int]$record.session_id -ne [int]$newSession.session_id -or $record.logon_identity_sha256 -cne $newSession.logon_identity_sha256)) { throw 'P028_TASK_SESSION_MISMATCH' }
        }
        if ($Phase -eq 'LOGON_TRIGGER_VERIFIED' -and $record.manual_task_run_present -ne $false) { throw 'P028_MANUAL_TASK_RUN_REJECTED' }
    }
    if ($Phase -eq 'EVIDENCE_FINALIZED' -and ($state.phases[-1].phase -cne 'CLEANUP_VERIFIED' -or $state.finalization_digest)) { throw 'P028_FINALIZATION_PRECONDITION' }
    $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($raw)).ToLowerInvariant()
    if (@($state.phases | Where-Object digest -ceq $digest).Count -gt 0) { throw 'P028_EVIDENCE_REPLAY' }
    $destination = Join-Path $evidenceRoot ($Phase + '.json')
    $destTemp = Join-Path $evidenceRoot ('.evidence-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $out = [IO.File]::Open($destTemp, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $out.Write($raw); $out.Flush($true) } finally { $out.Dispose() }
    if (Test-Path -LiteralPath $destination) {
        $existingDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([P028SafeEvidenceReader]::Read($destination, 4194304))).ToLowerInvariant()
        if ($existingDigest -cne $digest) { throw 'P028_EVIDENCE_REPLAY' }
        Remove-Item -LiteralPath $destTemp -Force
    } else { [IO.File]::Move($destTemp, $destination) }
    $now = [DateTimeOffset]::UtcNow.ToString('o')
    $state.phases += [ordered]@{phase=$Phase;digest=$digest;observer_sid_sha256=if($observerPhases -ccontains $Phase){$observerHash}else{$null};observed_at_utc=$observedAt.ToString('o');completed_at_utc=$now}
    if ($Phase -eq 'EVIDENCE_FINALIZED') { $state.finalization_digest=$digest; $state.status='FINALIZED' }
    Save-RunState $state
    Write-Output "P028_PHASE_RECORDED $Phase $digest"
} finally {
    if ($hasLock) { $mutex.ReleaseMutex() }
    if ($null -ne $mutex) { $mutex.Dispose() }
}
