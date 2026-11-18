# Portable checks of production validation functions on synthetic XML and process records.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-native-observer-support.ps1')
. (Join-Path $PSScriptRoot 'windows-native-logon-task.ps1')
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'protect-windows-native-run-state.ps1'), [ref]$null, [ref]$null)
foreach ($name in @('Get-P028Event','Resolve-P028Sid','Normalize-P028LogonId')) {
    $node = $ast.Find({param($item) $item -is [Management.Automation.Language.FunctionDefinitionAst] -and $item.Name -eq $name}, $true)
    Invoke-Expression $node.Extent.Text
}
$script:checks = 0
function Assert-Rejected([scriptblock]$Case, [string]$Reason) {
    try { & $Case | Out-Null } catch {
        if ($_.Exception.Message -cne $Reason) { throw }
        $script:checks++
        return
    }
    throw "TEST_EXPECTED_REJECTION: $Reason"
}
$subjectSid = 'S-1-5-21-101-102-103-1001'
$profileId = '01234567-89ab-7cde-8fab-0123456789ab'
$image = [IO.Path]::Combine([IO.Path]::GetTempPath(), 'Synveil', 'synveil-client.exe')
$taskContext = [pscustomobject]@{TaskPath="\Synveil\BackgroundClient\profile-$profileId"; ClientPath=$image}
$workingDirectory = [IO.Path]::GetDirectoryName($image)
$taskXml = @"
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>$subjectSid</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="SynveilUser"><UserId>$subjectSid</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Actions Context="SynveilUser"><Exec><Command>$image</Command><WorkingDirectory>$workingDirectory</WorkingDirectory></Exec></Actions>
</Task>
"@
Assert-P028TaskDefinition $taskXml $image $subjectSid
$script:checks++
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace('LogonTrigger','BootTrigger')) $image $subjectSid } 'P028_TASK_DEFINITION_INVALID'
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace('</Triggers>','<TimeTrigger/></Triggers>')) $image $subjectSid } 'P028_TASK_DEFINITION_INVALID'
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace('<Enabled>true','<Enabled>false')) $image $subjectSid } 'P028_TASK_LOGON_TRIGGER_MISMATCH'
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace('InteractiveToken','Password')) $image $subjectSid } 'P028_TASK_PRINCIPAL_MISMATCH'
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace("id=`"SynveilUser`"><UserId>$subjectSid","id=`"SynveilUser`"><UserId>S-1-5-18")) $image $subjectSid } 'P028_TASK_PRINCIPAL_MISMATCH'
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace('LeastPrivilege','HighestAvailable')) $image $subjectSid } 'P028_TASK_PRINCIPAL_MISMATCH'
Assert-Rejected { Assert-P028TaskDefinition ($taskXml.Replace('</Command>','</Command><Arguments>anything</Arguments>')) $image $subjectSid } 'P028_TASK_ACTION_MISMATCH'
Assert-Rejected { Assert-P028TaskDefinition $taskXml ($image + '.other') $subjectSid } 'P028_TASK_ACTION_MISMATCH'

$logon = [DateTimeOffset]'2026-11-01T12:00:00Z'
$now = $logon.AddMinutes(2)
$instanceGuid = '{83c9d541-5641-4027-88a0-2a813dd0d180}'
function New-TaskRecords {
    foreach ($eventSpec in @(
        @{Id=119; RecordId=101; Time=1; Data=@{InstanceId=$instanceGuid; UserName=$subjectSid}},
        @{Id=100; RecordId=103; Time=2; Data=@{InstanceId=$instanceGuid; UserContext=$subjectSid}},
        @{Id=129; RecordId=102; Time=1.5; Data=@{Path=$instanceGuid; ProcessID='3420'}},
        @{Id=200; RecordId=104; Time=3; Data=@{TaskInstanceId=$instanceGuid; ActionName=$image}}
    )) {
        $eventSpec.Data.TaskName = $taskContext.TaskPath
        [pscustomobject]@{Id=$eventSpec.Id; RecordId=$eventSpec.RecordId; Version=0; TimeUtc=$logon.AddSeconds($eventSpec.Time); Data=$eventSpec.Data}
    }
}
$records = @(New-TaskRecords)
$instance = Get-P028LogonTaskInstance $records $taskContext $subjectSid $logon $now 100
if ($instance.ProcessId -ne 3420 -or $instance.InstanceId -ine $instanceGuid.Trim('{}')) { throw 'TEST_WRONG_INSTANCE' }
$script:checks++
foreach ($case in @(
    @{Index=0; Alter={param($r) $r.Id=110}; Reason='P028_TASK_EXECUTION_MISSING_OR_AMBIGUOUS'},
    @{Index=0; Alter={param($r) $r.Id=107}; Reason='P028_TASK_EXECUTION_MISSING_OR_AMBIGUOUS'},
    @{Index=0; Alter={param($r) $r.Data.UserName='S-1-5-18'}; Reason='P028_TASK_INSTANCE_OR_IDENTITY_MISMATCH'},
    @{Index=1; Alter={param($r) $r.Data.TaskName+='-other'}; Reason='P028_TASK_EVENT_BINDING_OR_WINDOW_INVALID'},
    @{Index=1; Alter={param($r) $r.Data.InstanceId='{c72b4a21-14e9-42a2-b5a4-dd4344f4ebc9}'}; Reason='P028_TASK_INSTANCE_OR_IDENTITY_MISMATCH'},
    @{Index=1; Alter={param($r) $r.RecordId=100}; Reason='P028_TASK_EVENT_BINDING_OR_WINDOW_INVALID'},
    @{Index=1; Alter={param($r) $r.RecordId=101}; Reason='P028_TASK_EXECUTION_MISSING_OR_AMBIGUOUS'},
    @{Index=2; Alter={param($r) $r.Data.Path='not-a-guid'}; Reason='P028_TASK_INSTANCE_ID_INVALID'},
    @{Index=2; Alter={param($r) $r.Data.ProcessID='0'}; Reason='P028_TASK_PROCESS_ID_INVALID'},
    @{Index=2; Alter={param($r) $r.TimeUtc=$logon.AddMilliseconds(1)}; Reason='P028_TASK_EVENT_ORDER_INVALID'},
    @{Index=3; Alter={param($r) $r.Version=42}; Reason='P028_SCHEDULER_EVENT_SCHEMA_UNSUPPORTED'},
    @{Index=3; Alter={param($r) $r.TimeUtc=$logon.AddSeconds(61)}; Reason='P028_TASK_EVENT_BINDING_OR_WINDOW_INVALID'},
    @{Index=3; Alter={param($r) $r.RecordId=105; $r.TimeUtc=$logon.AddMilliseconds(-1)}; Reason='P028_TASK_EVENT_BINDING_OR_WINDOW_INVALID'}
)) {
    $changed = @(New-TaskRecords)
    & $case.Alter $changed[$case.Index]
    Assert-Rejected { Get-P028LogonTaskInstance $changed $taskContext $subjectSid $logon $now 100 } $case.Reason
}
Assert-Rejected { Get-P028LogonTaskInstance ($records + $records[0]) $taskContext $subjectSid $logon $now 100 } 'P028_TASK_EXECUTION_MISSING_OR_AMBIGUOUS'

$session = [pscustomobject]@{Sid=$subjectSid; SessionId=4; Luid='7ab'}
$processValues = @{Sid=$subjectSid; SessionId=4; Luid='0x7ab'; ImagePath=$image; Elevated=$false; Administrator=$false; CreatedUtc=$logon.AddSeconds(1.5)}
Assert-P028TaskProcess ([pscustomobject]$processValues) $instance $taskContext $session $logon
$script:checks++
foreach ($change in @{SessionId=5; Luid='0x7ac'; Sid='S-1-5-18'; ImagePath=($image + '.other'); Elevated=$true; Administrator=$true; CreatedUtc=$logon.AddSeconds(10)}.GetEnumerator()) {
    $otherProcess = $processValues.Clone()
    $otherProcess[$change.Key] = $change.Value
    Assert-Rejected { Assert-P028TaskProcess ([pscustomobject]$otherProcess) $instance $taskContext $session $logon } 'P028_TASK_PROCESS_IDENTITY_MISMATCH'
}

$newLogon = [pscustomobject]@{RecordId=500; TimeUtc=$logon}
$process = [pscustomobject]$processValues
$processAudit = [pscustomobject]@{Version=2; RecordId=501; TimeUtc=$process.CreatedUtc
    Data=@{NewProcessName=$image; TargetUserSid=$subjectSid; TargetLogonId='0x7ab'}}
Assert-P028ProcessAuditBinding $processAudit $process $newLogon $session
$script:checks++
$processAudit.Data.TargetLogonId = '0x777'
Assert-Rejected { Assert-P028ProcessAuditBinding $processAudit $process $newLogon $session } 'P028_TASK_PROCESS_AUDIT_MISMATCH'
$processAudit.Data.TargetLogonId = '0x7ab'
$processAudit.Version = 1
Assert-Rejected { Assert-P028ProcessAuditBinding $processAudit $process $newLogon $session } 'P028_TASK_PROCESS_AUDIT_MISMATCH'
$processAudit.Version = 2
$processAudit.RecordId = $newLogon.RecordId
Assert-Rejected { Assert-P028ProcessAuditBinding $processAudit $process $newLogon $session } 'P028_TASK_PROCESS_AUDIT_MISMATCH'

$computerSystem = @{Domain='WORKGROUP'}
$fakeEvent = [pscustomobject]@{Id=119; LogName=$P028SchedulerChannel; ProviderName='Microsoft-Windows-TaskScheduler'; RecordId=101; Version=0; TimeCreated=$logon.UtcDateTime}
$fakeEvent | Add-Member -MemberType NoteProperty -Name Xml -Value @"
<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><Provider Name="Microsoft-Windows-TaskScheduler" Guid="{DE7B24EA-73C8-4A09-985D-5BDADCFA9017}"/><EventID>119</EventID><Version>0</Version>
<TimeCreated SystemTime="2026-11-01T12:00:00.0000000Z"/><EventRecordID>101</EventRecordID>
<Channel>$P028SchedulerChannel</Channel><Computer>$([Environment]::MachineName)</Computer></System>
<EventData><Data Name="TaskName">$($taskContext.TaskPath)</Data><Data Name="UserName">$subjectSid</Data><Data Name="InstanceId">$instanceGuid</Data></EventData>
</Event>
"@
$fakeEvent | Add-Member -MemberType ScriptMethod -Name ToXml -Value { $this.Xml }
$reference = Get-P028Event $fakeEvent 119 $P028SchedulerChannel @('TaskName','UserName','InstanceId')
function Get-WinEvent {
    param($LogName, $FilterXPath, $MaxEvents)
    if ($LogName -cne $P028SchedulerChannel -or $FilterXPath -cne '*[System[EventRecordID=101]]' -or $MaxEvents -ne 2) {
        throw 'TEST_WRONG_REQUERY'
    }
    return $fakeEvent
}
Assert-P028SchedulerEventRequery $reference
$script:checks++
$validXml = $fakeEvent.Xml
$fakeEvent.Xml = $validXml.Replace($subjectSid, 'S-1-5-18')
Assert-Rejected { Assert-P028SchedulerEventRequery $reference } 'P028_TASK_EVENT_REQUERY_MISMATCH'
$fakeEvent.Xml = $validXml.Replace('<EventRecordID>101', '<EventRecordID>99')
Assert-Rejected { Assert-P028SchedulerEventRequery $reference } 'P028_EVENT_SCHEMA_MISMATCH'
$fakeEvent.Xml = $validXml.Replace("<Computer>$([Environment]::MachineName)", '<Computer>other-machine.invalid')
Assert-Rejected { Assert-P028SchedulerEventRequery $reference } 'P028_EVENT_MACHINE_MISMATCH'
$fakeEvent.Xml = $validXml.Replace('</EventData>', '<Data Name="InstanceId">duplicate</Data></EventData>')
Assert-Rejected { Assert-P028SchedulerEventRequery $reference } 'P028_EVENT_DATA_AMBIGUOUS'
$fakeEvent.Xml = $validXml
$fakeEvent.RecordId = 99
Assert-Rejected { Assert-P028SchedulerEventRequery $reference } 'P028_TASK_EVENT_REFERENCE_MISMATCH'

$RunId='run'; $SourceCommit='source'; $HarnessCommit='harness'; $CandidateSha256='candidate'; $ManifestSha256='manifest'
$runState = @{machine_guid_sha256='machine'; boot_time_sha256='boot'; controller_sid_sha256='controller'; subject_sid_sha256='subject'; observer_sid_sha256='observer'}
$baseline = $runState.Clone()
$baseline.schema_version=1; $baseline.run_id=$RunId; $baseline.source_commit=$SourceCommit; $baseline.harness_commit=$HarnessCommit
$baseline.candidate_sha256=$CandidateSha256; $baseline.manifest_sha256=$ManifestSha256
$baseline.client_sha256='a'*64; $baseline.definition_sha256='b'*64
Assert-P028BaselineContext $baseline $runState
$script:checks++
foreach ($field in @('run_id','source_commit','harness_commit','candidate_sha256','manifest_sha256','machine_guid_sha256','boot_time_sha256','observer_sid_sha256','subject_sid_sha256','controller_sid_sha256')) {
    $substitution = $baseline.Clone()
    $substitution[$field] = 'substitution'
    Assert-Rejected { Assert-P028BaselineContext $substitution $runState } 'P028_TASK_BASELINE_CONTEXT_MISMATCH'
}
Assert-P028AccessMask 0x1200A9 0x1200A9 0x0D0156
$script:checks++
Assert-Rejected { Assert-P028AccessMask 0x1301BF 0x1200A9 0x0D0156 } 'P028_ROLE_EFFECTIVE_ACCESS_MISMATCH'
Assert-Rejected { Assert-P028AccessMask 0x40 0 0x40 } 'P028_ROLE_EFFECTIVE_ACCESS_MISMATCH'
foreach ($invalid in @('C:\path\..\substitute','C:\path:stream','\\server\share','C:\path\NUL.txt','C:\path.\P028')) {
    Assert-Rejected { Assert-P028CanonicalPath $invalid } 'P028_PATH_NOT_CANONICAL'
}
Write-Output "P028_LOGON_TASK_FIXTURES_PASS ($script:checks cases; synthetic inputs)"

if ($IsWindows) {
    # Non-mutating hosted smoke checks. This probes metadata and the current token, with no interactive logon or task execution.
    $current = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        if ([P028ObserverAccess]::IsElevated([uint32]$PID, $current.User.Value) -isnot [bool]) { throw 'TEST_CURRENT_TOKEN_UNAVAILABLE' }
    } finally { $current.Dispose() }
    $scriptDigest = (Get-FileHash -LiteralPath $PSCommandPath).Hash.ToLowerInvariant()
    if ([P028SafeEvidenceReader]::Digest($PSCommandPath, 262144) -cne $scriptDigest) { throw 'TEST_PINNED_READ_DIGEST_MISMATCH' }
    $provider = Microsoft.PowerShell.Diagnostics\Get-WinEvent -ListProvider 'Microsoft-Windows-TaskScheduler' -ErrorAction Stop
    $knownFields = @{119=@('TaskName','InstanceId','UserName'); 110=@('TaskName','InstanceId','UserContext')
        100=@('TaskName','InstanceId','UserContext'); 129=@('TaskName','Path','ProcessID'); 200=@('TaskName','TaskInstanceId','ActionName')}
    foreach ($eventId in $knownFields.Keys) {
        $metadata = @($provider.Events | Where-Object { $_.Id -eq $eventId -and $_.Version -eq 0 })
        if ($metadata.Count -ne 1 -or [string]::IsNullOrWhiteSpace($metadata[0].Template)) { throw 'TEST_SCHEDULER_METADATA_UNAVAILABLE' }
        [xml]$template = $metadata[0].Template
        foreach ($field in $knownFields[$eventId]) {
            if ($field -cnotin @($template.DocumentElement.ChildNodes | ForEach-Object { $_.GetAttribute('name') })) {
                throw 'TEST_SCHEDULER_METADATA_FIELDS_CHANGED'
            }
        }
    }
    Write-Output 'P028_WINDOWS_TOKEN_PINNED_READ_AND_PROVIDER_METADATA_SMOKE_PASS'
}
