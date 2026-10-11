param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [string]$OlderFixtureSetup,
    [string]$NewerFixtureSetup,
    [string]$NewerLicenseHash
)

$ErrorActionPreference = 'Stop'
$user = 'synveil_p025_' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$password = [Guid]::NewGuid().ToString('N') + '!aA7'
Write-Output "::add-mask::$password"
$account = $null
$accountCreationAttempted = $false
$accountCreated = $false
$sid = $null
$stage = 'initialize'
$primaryFailure = $null
$primaryFailureMessage = $null
$cleanupFailure = $null
$cleanupFailureMessage = $null
$evidence = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-per-user-evidence.json'
$failureEvidence = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-per-user-failure.json'
$childEvidence = Join-Path $env:SystemDrive "Users\$user\AppData\Local\Synveil\installer\windows-per-user-evidence.json"
function Record-CleanupFailure([object]$ErrorRecord, [string]$CleanupStage) {
    if ($null -eq $script:cleanupFailure) {
        $script:cleanupFailure = $ErrorRecord
        $message = $ErrorRecord.Exception.Message
        if (![string]::IsNullOrEmpty($script:password)) { $message = $message.Replace($script:password,'[REDACTED]') }
        $script:cleanupFailureMessage = "$CleanupStage : $message"
    }
    Write-Host "STANDARD_USER_CLEANUP_FAILURE stage=$CleanupStage type=$($ErrorRecord.Exception.GetType().FullName)"
}
New-Item $EvidenceDirectory -ItemType Directory -Force | Out-Null
try {
    $stage = 'create_account'
    $computer = [ADSI]"WinNT://$env:COMPUTERNAME,computer"
    $accountCreationAttempted = $true
    $account = $computer.Create('user',$user)
    $account.SetPassword($password)
    $account.Put('Description','Disposable Synveil P025 standard-user acceptance account')
    $account.SetInfo()
    $accountCreated = $true
    $stage = 'resolve_account_identity'
    $sid = ([Security.Principal.NTAccount]::new($env:COMPUTERNAME,$user)).Translate([Security.Principal.SecurityIdentifier]).Value
    $admins = [ADSI]"WinNT://$env:COMPUTERNAME/Administrators,group"
    $isAdmin = @($admins.psbase.Invoke('Members')) | Where-Object { $_.GetType().InvokeMember('Name','GetProperty',$null,$_,$null) -eq $user }
    if ($isAdmin) { throw 'STANDARD_USER_SETUP_FAILURE: disposable account is an Administrator' }

    $secure = ConvertTo-SecureString $password -AsPlainText -Force
    $credential = [PSCredential]::new("$env:COMPUTERNAME\$user",$secure)
    $stdout = Join-Path $EvidenceDirectory 'standard-user.stdout.log'
    $stderr = Join-Path $EvidenceDirectory 'standard-user.stderr.log'
    $script = Join-Path $PSScriptRoot 'test-windows-per-user-installation.ps1'
    $stage = 'start_standard_user_test'
    $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$script,'-Setup',([IO.Path]::GetFullPath($Setup)),'-RepositoryRoot',([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))),'-EvidencePath',$childEvidence)
    $process = Start-Process (Get-Command pwsh).Source -Credential $credential -LoadUserProfile -ArgumentList $arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $stage = 'wait_for_standard_user_test'
    if (!$process.WaitForExit(300000)) { $process.Kill(); throw 'STANDARD_USER_TEST_FAILURE: child exceeded five-minute bound' }
    if ($process.ExitCode -ne 0) { throw "STANDARD_USER_TEST_FAILURE: child exit $($process.ExitCode); see bounded logs" }
    $stage = 'copy_standard_user_evidence'
    Copy-Item -LiteralPath $childEvidence -Destination $evidence
    if ($OlderFixtureSetup -and $NewerFixtureSetup) {
        if ($NewerLicenseHash -notmatch '^[0-9a-f]{64}$') { throw 'STANDARD_USER_TEST_FAILURE: newer fixture license identity is missing' }
        $lifecycleScript = Join-Path $PSScriptRoot 'test-windows-installer-lifecycle.ps1'
        $lifecycleEvidence = Join-Path $env:SystemDrive "Users\$user\AppData\Local\Synveil\installer\windows-lifecycle-evidence.json"
        $lifecycleArgs = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$lifecycleScript,
            '-Setup',([IO.Path]::GetFullPath($Setup)),'-OlderFixtureSetup',([IO.Path]::GetFullPath($OlderFixtureSetup)),
            '-NewerFixtureSetup',([IO.Path]::GetFullPath($NewerFixtureSetup)),'-NewerLicenseHash',$NewerLicenseHash,
            '-RepositoryRoot',([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))),
            '-EvidencePath',$lifecycleEvidence)
        $stage = 'start_lifecycle_test'
        $lifecycle = Start-Process (Get-Command pwsh).Source -Credential $credential -LoadUserProfile -ArgumentList $lifecycleArgs `
            -RedirectStandardOutput (Join-Path $EvidenceDirectory 'lifecycle.stdout.log') -RedirectStandardError (Join-Path $EvidenceDirectory 'lifecycle.stderr.log') -PassThru
        $stage = 'wait_for_lifecycle_test'
        if (!$lifecycle.WaitForExit(600000)) { $lifecycle.Kill(); throw 'STANDARD_USER_TEST_FAILURE: lifecycle child exceeded ten-minute bound' }
        if ($lifecycle.ExitCode -ne 0) { throw "STANDARD_USER_TEST_FAILURE: lifecycle child exit $($lifecycle.ExitCode); see bounded logs" }
        $stage = 'copy_lifecycle_evidence'
        Copy-Item -LiteralPath $lifecycleEvidence -Destination (Join-Path $EvidenceDirectory 'windows-lifecycle-evidence.json')
    }
} catch {
    $primaryFailure = $_
    $safeMessage = $_.Exception.Message
    if (![string]::IsNullOrEmpty($password)) { $safeMessage = $safeMessage.Replace($password,'[REDACTED]') }
    $primaryFailureMessage = $safeMessage
    $failureRecord = [ordered]@{
        schema_version = 1
        source_commit = $env:GITHUB_SHA
        test_stage = $stage
        account_name = $user
        account_created = $accountCreated
        account_sid = $sid
        result = 'FAIL'
        error_type = $_.Exception.GetType().FullName
        error_message = $safeMessage
        cleanup = 'pending'
    }
    try { $failureRecord | ConvertTo-Json | Set-Content -LiteralPath $failureEvidence -Encoding utf8 }
    catch { Write-Host "STANDARD_USER_EVIDENCE_WRITE_FAILURE type=$($_.Exception.GetType().FullName)" }
    Write-Host "STANDARD_USER_TEST_FAILURE stage=$stage type=$($primaryFailure.Exception.GetType().FullName) message=$safeMessage"
} finally {
    $childLogs = Join-Path $env:SystemDrive "Users\$user\AppData\Local\Synveil\installer"
    if (Test-Path $childLogs -PathType Container) {
        try { Copy-Item -LiteralPath $childLogs -Destination (Join-Path $EvidenceDirectory 'standard-user-logs') -Recurse -Force }
        catch { Record-CleanupFailure $_ 'copy_logs' }
    }
    if ($accountCreationAttempted) {
        try {
            $filterName = $user.Replace("'", "''")
            $localAccounts = @(Get-CimInstance -ClassName Win32_UserAccount -Filter "LocalAccount=TRUE AND Name='$filterName'" -ErrorAction Stop)
            if ($localAccounts.Count -gt 1) { throw 'STANDARD_USER_CLEANUP_FAILURE: generated local account name is not unique' }
            if ($localAccounts.Count -eq 1) {
                $localAccount = $localAccounts[0]
                $expectedDescription = 'Disposable Synveil P025 standard-user acceptance account'
                if (($sid -and $localAccount.SID -cne $sid) -or (!$sid -and $localAccount.Description -cne $expectedDescription)) {
                    throw 'STANDARD_USER_CLEANUP_FAILURE: refusing to delete an account with an unexpected identity'
                }
                ([ADSI]"WinNT://$env:COMPUTERNAME,computer").Delete('user',$user)
                $remainingAccounts = @(Get-CimInstance -ClassName Win32_UserAccount -Filter "LocalAccount=TRUE AND Name='$filterName'" -ErrorAction Stop)
                if ($remainingAccounts.Count -ne 0) { throw 'STANDARD_USER_CLEANUP_FAILURE: disposable account remains after deletion' }
            }
        } catch {
            Record-CleanupFailure $_ 'delete_account'
        }
    }
    if ($sid) {
        try {
            $profiles = @(Get-CimInstance Win32_UserProfile -Filter "SID='$sid'" -ErrorAction Stop)
            if ($profiles.Count -gt 0) { $profiles | Remove-CimInstance -ErrorAction Stop }
            if (@(Get-CimInstance Win32_UserProfile -Filter "SID='$sid'" -ErrorAction Stop).Count -ne 0) {
                throw 'STANDARD_USER_CLEANUP_FAILURE: disposable profile remains after removal'
            }
        } catch {
            Record-CleanupFailure $_ 'delete_profile'
        }
    }
    $recordPath = if (Test-Path $evidence) { $evidence } elseif (Test-Path $failureEvidence) { $failureEvidence } else { $null }
    if ($recordPath) {
        try {
            $record = Get-Content $recordPath -Raw | ConvertFrom-Json
            $record.cleanup = if ($null -eq $cleanupFailure) { 'pass' } else { 'fail' }
            if ($null -ne $cleanupFailure) { $record.cleanup_error = $cleanupFailureMessage }
            $record | ConvertTo-Json | Set-Content -LiteralPath $recordPath -Encoding utf8
        } catch {
            Record-CleanupFailure $_ 'write_cleanup_status'
        }
    }
    $password = $null
}
if ($null -ne $primaryFailure) {
    if ($null -ne $cleanupFailure) {
        Write-Host "STANDARD_USER_CLEANUP_FAILURE secondary_to_test_failure details=$cleanupFailureMessage"
    }
    throw "STANDARD_USER_TEST_FAILURE stage=$stage type=$($primaryFailure.Exception.GetType().FullName) message=$primaryFailureMessage"
}
if ($null -ne $cleanupFailure) {
    throw "STANDARD_USER_CLEANUP_FAILURE details=$cleanupFailureMessage"
}
if (!(Test-Path $evidence)) { throw 'STANDARD_USER_TEST_FAILURE: evidence record missing' }
Write-Host 'Disposable standard-user acceptance: PASS (account and profile removed)'
