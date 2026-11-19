# Private, read-only scheduler and process queries. Results are observations with no phase authority.
$P028SchedulerChannel = 'Microsoft-Windows-TaskScheduler/Operational'
$P028SchedulerProofIds = @(119,100,129,200)
$P028SchedulerConflictIds = @(101,102,103,106,107,108,109,110,111,114,117,118,120,121,122,123,124,125,126,127,140,141,142,145,201,203,322)
$P028TaskBaselineFile = 'P028-LOGON-TASK-BASELINE-v1.json'
$P028TaskObservationFile = 'P028-LOGON-TASK-OBSERVATION-v1.json'

function Assert-P028TaskDefinition([string]$XmlText, [string]$ClientPath, [string]$Subject) {
    if ($XmlText.Length -gt 65536 -or $XmlText -match '<!DOCTYPE') { throw 'P028_TASK_DEFINITION_INVALID' }
    [xml]$xml = $XmlText
    $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
    $ns.AddNamespace('t','http://schemas.microsoft.com/windows/2004/02/mit/task')
    $triggers = $xml.SelectNodes('/t:Task/t:Triggers/*',$ns)
    $principals = $xml.SelectNodes('/t:Task/t:Principals/*',$ns)
    $actions = $xml.SelectNodes('/t:Task/t:Actions/*',$ns)
    if ($triggers.Count -ne 1 -or $triggers[0].LocalName -cne 'LogonTrigger' -or
        $principals.Count -ne 1 -or $principals[0].LocalName -cne 'Principal' -or
        $actions.Count -ne 1 -or $actions[0].LocalName -cne 'Exec') { throw 'P028_TASK_DEFINITION_INVALID' }
    $triggerUser = $xml.SelectNodes('/t:Task/t:Triggers/t:LogonTrigger/t:UserId',$ns)
    $triggerEnabled = $xml.SelectNodes('/t:Task/t:Triggers/t:LogonTrigger/t:Enabled',$ns)
    if ($triggerUser.Count -ne 1 -or $triggerEnabled.Count -ne 1 -or
        $triggerEnabled[0].InnerText -cne 'true' -or (Resolve-P028Sid $triggerUser[0].InnerText) -cne $Subject) {
        throw 'P028_TASK_LOGON_TRIGGER_MISMATCH'
    }
    $principal = $principals[0]
    if ((Resolve-P028Sid ([string]$principal.UserId)) -cne $Subject -or
        [string]$principal.LogonType -cne 'InteractiveToken' -or [string]$principal.RunLevel -cne 'LeastPrivilege' -or
        $principal.GetAttribute('id') -cne 'SynveilUser' -or $actions[0].ParentNode.GetAttribute('Context') -cne 'SynveilUser' -or
        $xml.SelectNodes('//t:Password | //t:GroupId',$ns).Count -ne 0) { throw 'P028_TASK_PRINCIPAL_MISMATCH' }
    $action = $actions[0]
    if ($action.NamespaceURI -cne $ns.LookupNamespace('t') -or $principal.NamespaceURI -cne $action.NamespaceURI -or
        $triggers[0].NamespaceURI -cne $action.NamespaceURI -or
        [string]$action.Command -ine $ClientPath -or [string]$action.WorkingDirectory -ine [IO.Path]::GetDirectoryName($ClientPath) -or
        $action.SelectNodes('t:Arguments',$ns).Count -ne 0) { throw 'P028_TASK_ACTION_MISMATCH' }
}

function Get-P028TaskContext([string]$ProfileId) {
    if ($ProfileId -cnotmatch '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') { throw 'P028_TASK_PROFILE_ID_INVALID' }
    $registryPath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\' + $subject.Value
    $profile = Assert-P028CanonicalPath ([string](Get-ItemPropertyValue -LiteralPath $registryPath -Name ProfileImagePath))
    $client = Join-Path $profile 'AppData\Local\Programs\Synveil\synveil-client.exe'
    $service = New-Object -ComObject 'Schedule.Service'
    $service.Connect()
    $folder = $service.GetFolder('\Synveil\BackgroundClient')
    $task = $folder.GetTask('profile-' + $ProfileId)
    $taskPath = '\Synveil\BackgroundClient\profile-' + $ProfileId
    if ($task.Path -ine $taskPath -or -not $task.Enabled) { throw 'P028_TASK_IDENTITY_MISMATCH' }
    $definition = [string]$task.Xml
    Assert-P028TaskDefinition $definition $client $subject.Value
    return [pscustomobject]@{TaskPath=$taskPath; ClientPath=$client; DefinitionSha256=(Hash-P028Text $definition)}
}

function Get-P028SchedulerEventByRecordId([long]$RecordId, [int]$EventId = 0) {
    if ($RecordId -le 0) { throw 'P028_TASK_EVENT_REFERENCE_INVALID' }
    $events = @(Get-WinEvent -LogName $P028SchedulerChannel -FilterXPath "*[System[EventRecordID=$RecordId]]" -MaxEvents 2 -ErrorAction Stop)
    if ($events.Count -ne 1 -or $events[0].RecordId -ne $RecordId -or ($EventId -ne 0 -and $events[0].Id -ne $EventId)) { throw 'P028_TASK_EVENT_REFERENCE_MISMATCH' }
    return Get-P028Event $events[0] $events[0].Id $P028SchedulerChannel @()
}

function Assert-P028SchedulerEventRequery($Reference) {
    $actual = Get-P028SchedulerEventByRecordId $Reference.RecordId $Reference.Id
    if ($actual.XmlSha256 -cne $Reference.XmlSha256) { throw 'P028_TASK_EVENT_REQUERY_MISMATCH' }
}

function Read-P028TaskBaseline($RunState) {
    $path = Join-Path $trustedPath $P028TaskBaselineFile
    [void](Assert-NoReparsePath $path)
    $baseline = [Text.Encoding]::UTF8.GetString([P028SafeEvidenceReader]::Read($path, 65536)) | ConvertFrom-Json
    Assert-P028BaselineContext $baseline $RunState
    return $baseline
}

function Assert-P028BaselineContext($baseline, $RunState) {
    if ($baseline.schema_version -ne 1 -or $baseline.run_id -cne $RunId -or
        $baseline.source_commit -cne $SourceCommit -or $baseline.harness_commit -cne $HarnessCommit -or
        $baseline.candidate_sha256 -cne $CandidateSha256 -or $baseline.manifest_sha256 -cne $ManifestSha256 -or
        $baseline.machine_guid_sha256 -cne $RunState.machine_guid_sha256 -or $baseline.boot_time_sha256 -cne $RunState.boot_time_sha256 -or
        $baseline.controller_sid_sha256 -cne $RunState.controller_sid_sha256 -or $baseline.subject_sid_sha256 -cne $RunState.subject_sid_sha256 -or
        $baseline.observer_sid_sha256 -cne $RunState.observer_sid_sha256 -or
        $baseline.client_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        $baseline.client_file_identity_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        $baseline.definition_sha256 -cnotmatch '^[0-9a-f]{64}$') {
        throw 'P028_TASK_BASELINE_CONTEXT_MISMATCH'
    }
}

function Save-P028LogonTaskBaseline($RunState) {
    if (-not $TaskProfileId -or -not $CandidateClientSha256) { throw 'P028_TASK_BASELINE_INPUTS_REQUIRED' }
    $channel = Get-WinEvent -ListLog $P028SchedulerChannel -ErrorAction Stop
    if (-not $channel.IsEnabled) { throw 'P028_TASK_CHANNEL_DISABLED' }
    $latest = Get-WinEvent -LogName $P028SchedulerChannel -MaxEvents 1 -ErrorAction Stop
    $anchor = Get-P028Event $latest $latest.Id $P028SchedulerChannel @()
    $taskContext = Get-P028TaskContext $TaskProfileId
    [void](Assert-NoReparsePath $taskContext.ClientPath)
    $clientInspection = [P028SafeEvidenceReader]::Inspect($taskContext.ClientPath, 134217728)
    if ($clientInspection.Sha256 -cne $CandidateClientSha256) {
        throw 'P028_INSTALLED_CLIENT_DIGEST_MISMATCH'
    }
    $baseline = [ordered]@{
        schema_version=1; run_id=$RunId; source_commit=$SourceCommit; harness_commit=$HarnessCommit
        candidate_sha256=$CandidateSha256; manifest_sha256=$ManifestSha256; machine_guid_sha256=$RunState.machine_guid_sha256
        boot_time_sha256=$RunState.boot_time_sha256; controller_sid_sha256=$RunState.controller_sid_sha256
        subject_sid_sha256=$RunState.subject_sid_sha256; observer_sid_sha256=$RunState.observer_sid_sha256
        captured_at_utc=[DateTimeOffset]::UtcNow.ToString('o')
        profile_id=$TaskProfileId; definition_sha256=$taskContext.DefinitionSha256; client_sha256=$CandidateClientSha256
        client_file_identity_sha256=$clientInspection.FileIdentitySha256
        anchor_id=$anchor.Id; anchor_record_id=$anchor.RecordId; anchor_sha256=$anchor.XmlSha256
    }
    Save-P028ObserverRecord $baseline $P028TaskBaselineFile $trustedPath
}

function Get-P028SchedulerWindow($TaskContext, [long]$AnchorRecordId) {
    # Match names case-insensitively after the bounded query so differently cased task names cannot hide conflicts.
    $taskPath = $TaskContext.TaskPath
    if ($taskPath -cnotmatch '^\\Synveil\\BackgroundClient\\profile-[0-9a-f-]{36}$' -or $AnchorRecordId -le 0) {
        throw 'P028_TASK_QUERY_CONTEXT_INVALID'
    }
    # Use one numeric predicate per query group to stay within the Event Log XPath complexity limit.
    $records = [Collections.Generic.List[object]]::new()
    $total = 0
    $ids = $P028SchedulerProofIds + $P028SchedulerConflictIds
    for ($index = 0; $index -lt $ids.Count; $index += 8) {
        $last = [Math]::Min($index + 7, $ids.Count - 1)
        $eventIds = (($ids[$index..$last] | ForEach-Object { "EventID=$_" }) -join ' or ')
        $filter = "*[System[EventRecordID > $AnchorRecordId and ($eventIds)]]"
        try { $events = @(Get-WinEvent -LogName $P028SchedulerChannel -FilterXPath $filter -MaxEvents 4097 -ErrorAction Stop) }
        catch { if ($_.FullyQualifiedErrorId -notlike 'NoMatchingEventsFound,*') { throw }; $events = @() }
        $total += $events.Count
        if ($total -gt 4096) { throw 'P028_TASK_EVENT_QUERY_TRUNCATED' }
        foreach ($event in $events) {
            $record = Get-P028Event $event $event.Id $P028SchedulerChannel @('TaskName')
            if ($record.Data.TaskName -ieq $taskPath) { $records.Add($record) }
        }
    }
    return $records.ToArray()
}

function Get-P028LogonTaskInstance([object[]]$Events, $TaskContext, [string]$Subject, [DateTimeOffset]$LogonUtc,
    [DateTimeOffset]$Now, [long]$AnchorRecordId) {
    if ($Events.Count -ne 4 -or @($Events.RecordId | Select-Object -Unique).Count -ne 4 -or
        @($Events.Id | Select-Object -Unique).Count -ne 4 -or
        @($Events | Where-Object { $_.Id -notin $P028SchedulerProofIds }).Count -ne 0) { throw 'P028_TASK_EXECUTION_MISSING_OR_AMBIGUOUS' }
    $byId = @{}
    foreach ($record in $Events) {
        if ($record.Data.TaskName -ine $TaskContext.TaskPath -or $record.RecordId -le $AnchorRecordId -or
            $record.TimeUtc -lt $LogonUtc -or $record.TimeUtc -gt $Now -or ($record.TimeUtc - $LogonUtc).TotalSeconds -gt 60) {
            throw 'P028_TASK_EVENT_BINDING_OR_WINDOW_INVALID'
        }
        if ($record.Version -ne 0 -and -not ($record.Id -eq 200 -and $record.Version -eq 1)) {
            throw 'P028_SCHEDULER_EVENT_SCHEMA_UNSUPPORTED'
        }
        $byId[$record.Id] = $record
    }
    # On the checked Windows 11 24H2 publisher metadata, event 119 identifies
    # a user-logon-triggered task. Event 107 is a time trigger and event 110 a
    # user-initiated run; both remain conflict events and cannot satisfy this chain.
    $trigger = $byId[119]; $start = $byId[100]; $created = $byId[129]; $action = $byId[200]
    foreach ($record in @($start,$created,$action)) {
        if ($record.RecordId -le $trigger.RecordId -or $record.TimeUtc -lt $trigger.TimeUtc) { throw 'P028_TASK_EVENT_ORDER_INVALID' }
    }
    if ($action.RecordId -le $start.RecordId -or $action.TimeUtc -lt $start.TimeUtc -or
        $action.RecordId -le $created.RecordId -or $action.TimeUtc -lt $created.TimeUtc) { throw 'P028_TASK_EVENT_ORDER_INVALID' }
    $guids = foreach ($value in @($trigger.Data.InstanceId,$start.Data.InstanceId,$created.Data.Path,$action.Data.TaskInstanceId)) {
        $id = [Guid]::Empty
        if ($value -notmatch '^(?:\{[0-9a-f-]{36}\}|[0-9a-f-]{36})$' -or
            -not [Guid]::TryParse($value, [ref]$id) -or $id -eq [Guid]::Empty) { throw 'P028_TASK_INSTANCE_ID_INVALID' }
        $id.ToString('D')
    }
    if (@($guids | Select-Object -Unique).Count -ne 1 -or
        (Resolve-P028Sid $trigger.Data.UserName) -cne $Subject -or (Resolve-P028Sid $start.Data.UserContext) -cne $Subject -or
        $action.Data.ActionName -ine $TaskContext.ClientPath) { throw 'P028_TASK_INSTANCE_OR_IDENTITY_MISMATCH' }
    $processId = [uint32]0
    if ($created.Data.ProcessID -notmatch '^[0-9]{1,10}$' -or
        -not [uint32]::TryParse($created.Data.ProcessID, [ref]$processId) -or $processId -eq 0) { throw 'P028_TASK_PROCESS_ID_INVALID' }
    return [pscustomobject]@{InstanceId=$guids[0]; ProcessId=$processId; Trigger=$trigger; ProcessEvent=$created; Events=@($trigger,$start,$created,$action)}
}

function Assert-P028TaskProcess($Process, $Instance, $TaskContext, $Session, [DateTimeOffset]$LogonUtc) {
    if ($Process.ImagePath -ine $TaskContext.ClientPath -or $Process.Sid -cne $Session.Sid -or $Process.Elevated -or $Process.Administrator -or
        $Process.SessionId -ne $Session.SessionId -or (Normalize-P028LogonId $Process.Luid) -cne $Session.Luid -or
        $Process.CreatedUtc -lt $LogonUtc -or $Process.CreatedUtc -lt $Instance.Trigger.TimeUtc -or
        [Math]::Abs(($Process.CreatedUtc - $Instance.ProcessEvent.TimeUtc).TotalSeconds) -gt 2) {
        throw 'P028_TASK_PROCESS_IDENTITY_MISMATCH'
    }
}

function Get-P028ProcessAudit([uint32]$ProcessId, $Process, $Logon, $Session) {
    $events = Get-P028SecurityEvents 4688 $Logon.TimeUtc.UtcDateTime
    $matches = @(foreach ($event in $events) {
        $entry = Get-P028Event $event 4688 'Security' @('NewProcessId')
        if ((Normalize-P028LogonId $entry.Data.NewProcessId) -ceq $ProcessId.ToString('x')) { $entry }
    })
    if ($matches.Count -ne 1) { throw 'P028_TASK_PROCESS_AUDIT_MISSING_OR_AMBIGUOUS' }
    $record = $matches[0]
    Assert-P028ProcessAuditBinding $record $Process $Logon $Session
    $requery = @(Get-WinEvent -LogName 'Security' -FilterXPath "*[System[EventRecordID=$($record.RecordId)]]" -MaxEvents 2 -ErrorAction Stop)
    if ($requery.Count -ne 1 -or $requery[0].RecordId -ne $record.RecordId -or
        (Get-P028Event $requery[0] 4688 'Security' @('NewProcessId')).XmlSha256 -cne $record.XmlSha256) {
        throw 'P028_TASK_PROCESS_AUDIT_REQUERY_MISMATCH'
    }
    return $record
}

function Assert-P028ProcessAuditBinding($Record, $Process, $Logon, $Session) {
    if ($Record.Version -ne 2 -or $Record.RecordId -le $Logon.RecordId -or $Record.TimeUtc -lt $Logon.TimeUtc -or
        [Math]::Abs(($Record.TimeUtc - $Process.CreatedUtc).TotalSeconds) -gt 2 -or
        $Record.Data.NewProcessName -ine $Process.ImagePath -or $Record.Data.TargetUserSid -cne $Session.Sid -or
        (Normalize-P028LogonId $Record.Data.TargetLogonId) -cne $Session.Luid) {
        throw 'P028_TASK_PROCESS_AUDIT_MISMATCH'
    }
}

function Invoke-P028LogonTaskObservation($RunState, $NewLogon, [bool]$Verify) {
    $baseline = Read-P028TaskBaseline $RunState
    $logoff = Read-P028ObserverRecord 'P028-LOGOFF-v1.json' 'ObserveLogoff' $RunState
    $logoffEvent = Get-P028SecurityEventByRecordId 4634 ([long]$logoff.Record.event.record_id)
    $newLogonEvent = Get-P028SecurityEventByRecordId 4624 ([long]$NewLogon.Record.event.record_id)
    $captured = ([DateTimeOffset]$baseline.captured_at_utc)
    if ($captured -lt ([DateTimeOffset]$RunState.created_at_utc) -or $captured -ge $logoffEvent.TimeUtc -or
        [DateTimeOffset]::UtcNow - $captured -gt [TimeSpan]::FromMinutes(15)) { throw 'P028_TASK_BASELINE_WINDOW_INVALID' }
    $anchor = Get-P028SchedulerEventByRecordId ([long]$baseline.anchor_record_id) ([int]$baseline.anchor_id)
    if ($anchor.XmlSha256 -cne $baseline.anchor_sha256 -or $anchor.TimeUtc -gt $captured) { throw 'P028_TASK_ANCHOR_MISMATCH' }
    if (-not (Get-WinEvent -ListLog $P028SchedulerChannel -ErrorAction Stop).IsEnabled) { throw 'P028_TASK_CHANNEL_DISABLED' }
    $taskContext = Get-P028TaskContext $baseline.profile_id
    if ($taskContext.DefinitionSha256 -cne $baseline.definition_sha256) { throw 'P028_TASK_DEFINITION_CHANGED' }
    $events = @(Get-P028SchedulerWindow $taskContext $anchor.RecordId)
    $instance = Get-P028LogonTaskInstance $events $taskContext $subject.Value $newLogonEvent.TimeUtc ([DateTimeOffset]::UtcNow) $anchor.RecordId
    $session = @(Get-P028SubjectSession $subject.Value)[0]
    if ($session.SessionId -ne $NewLogon.Record.session.windows_session_id -or
        $session.LuidSha256 -cne $NewLogon.Record.session.authentication_luid_sha256 -or
        $session.Luid -cne (Normalize-P028LogonId $newLogonEvent.Data.TargetLogonId)) { throw 'P028_TASK_LOGON_SESSION_MISMATCH' }
    $logons = Get-P028SecurityEvents 4624 $logoffEvent.TimeUtc.UtcDateTime
    foreach ($event in $logons) {
        $actual = Get-P028Event $event 4624 'Security' @('TargetUserSid','TargetLogonId','LogonType')
        if ($actual.RecordId -ne $newLogonEvent.RecordId -and $actual.Data.TargetUserSid -ceq $subject.Value -and
            $actual.Data.LogonType -in @('2','10','11','12')) { throw 'P028_TASK_LOGON_CAUSE_AMBIGUOUS' }
    }
    $process = [P028ObserverNative]::ProcessToken($instance.ProcessId, $subject.Value)
    Assert-P028TaskProcess $process $instance $taskContext $session $newLogonEvent.TimeUtc
    $processAudit = Get-P028ProcessAudit $instance.ProcessId $process $newLogonEvent $session
    [void](Assert-NoReparsePath $taskContext.ClientPath)
    $clientInspection = [P028SafeEvidenceReader]::Inspect($taskContext.ClientPath, 134217728)
    if ($clientInspection.Sha256 -cne $baseline.client_sha256 -or
        $clientInspection.FileIdentitySha256 -cne $baseline.client_file_identity_sha256) {
        throw 'P028_INSTALLED_CLIENT_IDENTITY_CHANGED'
    }
    $references = foreach ($event in $instance.Events) {
        Assert-P028SchedulerEventRequery $event
        [ordered]@{event_id=$event.Id; record_id=$event.RecordId; record_sha256=$event.XmlSha256; occurred_at_filetime_utc=$event.TimeUtc.ToFileTime()}
    }
    $details = [ordered]@{
        task_sha256=(Hash-P028Text $taskContext.TaskPath); definition_sha256=$baseline.definition_sha256
        candidate_client_sha256=$baseline.client_sha256
        client_file_identity_sha256=$clientInspection.FileIdentitySha256
        mapped_image_identity='UNAVAILABLE_FROM_SUPPORTED_USER_MODE_API'
        instance_sha256=(Hash-P028Text $instance.InstanceId)
        process_id=$instance.ProcessId; process_created_filetime_utc=$process.CreatedUtc.ToFileTime(); scheduler_records=@($references)
        process_audit=[ordered]@{event_id=4688; record_id=$processAudit.RecordId; record_sha256=$processAudit.XmlSha256
            occurred_at_filetime_utc=$processAudit.TimeUtc.ToFileTime()}
        trigger_kind='TASK_SCHEDULER_LOGON_TRIGGER_EVENT_119'
        causality_status='BLOCKED_TASK_EVENT_HAS_NO_AUTHENTICATION_LUID'
        mapped_image_status='BLOCKED_MAPPED_IMAGE_PROVENANCE_UNAVAILABLE'
        status='BLOCKED_NATIVE_LOGON_TRIGGER_UNVERIFIED'
    }
    if ($Verify) {
        $stored = (Read-P028ObserverRecord $P028TaskObservationFile 'ObserveLogonTrigger' $RunState).Record
        if (@($stored.prior_record_sha256).Count -ne 1 -or $stored.prior_record_sha256[0] -cne $NewLogon.RecordDigest -or
            $stored.session.windows_session_id -ne $session.SessionId -or
            $stored.session.authentication_luid_sha256 -cne $session.LuidSha256 -or
            $stored.session.user_sid_sha256 -cne $RunState.subject_sid_sha256 -or
            $stored.event.record_id -ne $instance.Trigger.RecordId -or $stored.event.record_sha256 -cne $instance.Trigger.XmlSha256 -or
            ($stored.task | ConvertTo-Json -Depth 8 -Compress) -cne ($details | ConvertTo-Json -Depth 8 -Compress)) {
            throw 'P028_TASK_OBSERVATION_REQUERY_MISMATCH'
        }
        Write-Output 'P028_TASK_OBSERVATION_REQUERIED'
        return
    }
    $record = New-P028ObserverRecord $RunState 'ObserveLogonTrigger' $instance.Trigger $session @($NewLogon)
    $record.task = $details
    Save-P028ObserverRecord $record $P028TaskObservationFile
    Write-Output 'P028_TASK_OBSERVATION_RECORDED'
}
