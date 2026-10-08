param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [string]$OlderFixtureSetup,
    [string]$NewerFixtureSetup,
    [string]$NewerLicenseHash
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$sourceCommit = [string](git -C $repositoryRoot rev-parse HEAD)
if ($LASTEXITCODE -ne 0 -or $sourceCommit -notmatch '^[0-9a-f]{40}$') {
    throw 'STANDARD_USER_SOURCE_FAILURE: checkout identity unavailable'
}
# Local SAM account names must fit within 20 characters (16 here).
$user = 'sv_p025_' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$password = [Guid]::NewGuid().ToString('N') + '!aA7'
Write-Output "::add-mask::$password"
$account = $null
$accountCreated = $false
$sid = $null
$evidence = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-per-user-evidence.json'
$childEvidence = Join-Path $env:SystemDrive "Users\$user\AppData\Local\Synveil\installer\windows-per-user-evidence.json"
New-Item $EvidenceDirectory -ItemType Directory -Force | Out-Null
try {
    $computer = [ADSI]"WinNT://$env:COMPUTERNAME,computer"
    $account = $computer.Create('user',$user)
    $account.SetPassword($password)
    $account.Put('Description','Disposable Synveil P025 standard-user acceptance account')
    $account.SetInfo()
    $accountCreated = $true
    $sid = ([Security.Principal.NTAccount]::new($env:COMPUTERNAME,$user)).Translate([Security.Principal.SecurityIdentifier]).Value
    $admins = [ADSI]"WinNT://$env:COMPUTERNAME/Administrators,group"
    $isAdmin = @($admins.psbase.Invoke('Members')) | Where-Object { $_.GetType().InvokeMember('Name','GetProperty',$null,$_,$null) -eq $user }
    if ($isAdmin) { throw 'STANDARD_USER_SETUP_FAILURE: disposable account is an Administrator' }

    $secure = ConvertTo-SecureString $password -AsPlainText -Force
    $credential = [PSCredential]::new("$env:COMPUTERNAME\$user",$secure)
    $stdout = Join-Path $EvidenceDirectory 'standard-user.stdout.log'
    $stderr = Join-Path $EvidenceDirectory 'standard-user.stderr.log'
    $script = Join-Path $PSScriptRoot 'test-windows-per-user-installation.ps1'
    $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$script,'-Setup',([IO.Path]::GetFullPath($Setup)),'-RepositoryRoot',$repositoryRoot,'-EvidencePath',$childEvidence,'-SourceCommit',$sourceCommit,'-ExpectedSid',$sid)
    $process = Start-Process (Get-Command pwsh).Source -Credential $credential -LoadUserProfile -ArgumentList $arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    if (!$process.WaitForExit(300000)) { $process.Kill(); throw 'STANDARD_USER_TEST_FAILURE: child exceeded five-minute bound' }
    if ($process.ExitCode -ne 0) { throw "STANDARD_USER_TEST_FAILURE: child exit $($process.ExitCode); see bounded logs" }
    Copy-Item -LiteralPath $childEvidence -Destination $evidence
    if ($OlderFixtureSetup -and $NewerFixtureSetup) {
        if ($NewerLicenseHash -notmatch '^[0-9a-f]{64}$') { throw 'STANDARD_USER_TEST_FAILURE: newer fixture license identity is missing' }
        $lifecycleScript = Join-Path $PSScriptRoot 'test-windows-installer-lifecycle.ps1'
        $lifecycleEvidence = Join-Path $env:SystemDrive "Users\$user\AppData\Local\Synveil\installer\windows-lifecycle-evidence.json"
        $lifecycleArgs = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$lifecycleScript,
            '-Setup',([IO.Path]::GetFullPath($Setup)),'-OlderFixtureSetup',([IO.Path]::GetFullPath($OlderFixtureSetup)),
            '-NewerFixtureSetup',([IO.Path]::GetFullPath($NewerFixtureSetup)),'-NewerLicenseHash',$NewerLicenseHash,
            '-RepositoryRoot',$repositoryRoot,'-ExpectedSid',$sid,
            '-EvidencePath',$lifecycleEvidence,'-SourceCommit',$sourceCommit)
        $lifecycle = Start-Process (Get-Command pwsh).Source -Credential $credential -LoadUserProfile -ArgumentList $lifecycleArgs `
            -RedirectStandardOutput (Join-Path $EvidenceDirectory 'lifecycle.stdout.log') -RedirectStandardError (Join-Path $EvidenceDirectory 'lifecycle.stderr.log') -PassThru
        if (!$lifecycle.WaitForExit(600000)) { $lifecycle.Kill(); throw 'STANDARD_USER_TEST_FAILURE: lifecycle child exceeded ten-minute bound' }
        if ($lifecycle.ExitCode -ne 0) { throw "STANDARD_USER_TEST_FAILURE: lifecycle child exit $($lifecycle.ExitCode); see bounded logs" }
        Copy-Item -LiteralPath $lifecycleEvidence -Destination (Join-Path $EvidenceDirectory 'windows-lifecycle-evidence.json')
    }
} finally {
    $childLogs = Join-Path $env:SystemDrive "Users\$user\AppData\Local\Synveil\installer"
    if (Test-Path $childLogs -PathType Container) {
        Copy-Item -LiteralPath $childLogs -Destination (Join-Path $EvidenceDirectory 'standard-user-logs') -Recurse -Force
    }
    if ($accountCreated) {
        ([ADSI]"WinNT://$env:COMPUTERNAME,computer").Delete('user',$user)
    }
    if ($sid) {
        Get-CimInstance Win32_UserProfile -Filter "SID='$sid'" -ErrorAction SilentlyContinue | Remove-CimInstance -ErrorAction SilentlyContinue
    }
    if (Test-Path $evidence) {
        $record = Get-Content $evidence -Raw | ConvertFrom-Json
        $record.cleanup = 'pass'
        $record | ConvertTo-Json | Set-Content $evidence -Encoding utf8
    }
    $password = $null
}
if (!(Test-Path $evidence)) { throw 'STANDARD_USER_TEST_FAILURE: evidence record missing' }
Write-Host 'Disposable standard-user acceptance: PASS (account and profile removed)'
