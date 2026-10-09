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
$loadedProfile = $null
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
    $loadedProfile = Get-CimInstance Win32_UserProfile -Filter "SID='$sid'" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (!$loadedProfile -or !$loadedProfile.LocalPath) { throw 'STANDARD_USER_PROFILE_FAILURE: loaded profile path unavailable' }
    $childEvidence = Join-Path $loadedProfile.LocalPath 'AppData\Local\Synveil\installer\windows-per-user-evidence.json'
    Copy-Item -LiteralPath $childEvidence -Destination $evidence
    if ($OlderFixtureSetup -and $NewerFixtureSetup) {
        if ($NewerLicenseHash -notmatch '^[0-9a-f]{64}$') { throw 'STANDARD_USER_TEST_FAILURE: newer fixture license identity is missing' }
        $lifecycleScript = Join-Path $PSScriptRoot 'test-windows-installer-lifecycle.ps1'
        $lifecycleEvidence = Join-Path $loadedProfile.LocalPath 'AppData\Local\Synveil\installer\windows-lifecycle-evidence.json'
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
    if (!$loadedProfile -and $sid) {
        $loadedProfile = Get-CimInstance Win32_UserProfile -Filter "SID='$sid'" -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    $childLogs = $null
    if ($loadedProfile -and $loadedProfile.LocalPath) {
        $childLogs = Join-Path $loadedProfile.LocalPath 'AppData\Local\Synveil\installer'
    }
    if ($childLogs -and (Test-Path $childLogs -PathType Container)) {
        $boundedLogs = Join-Path $EvidenceDirectory 'standard-user-logs'
        New-Item $boundedLogs -ItemType Directory -Force | Out-Null
        $allowedLogs = @('install.log','uninstall.log','reinstall.log','uninstall-reinstall.log',
            'desktop.stdout.log','desktop.stderr.log','client.stdout.log','client.stderr.log',
            'windows-per-user-evidence.json','windows-lifecycle-evidence.json')
        foreach ($name in $allowedLogs) {
            $source = Join-Path $childLogs $name
            if (Test-Path $source -PathType Leaf) {
                $item = Get-Item -LiteralPath $source -Force
                if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) {
                    if ($item.Length -le 262144) {
                        Copy-Item -LiteralPath $source -Destination $boundedLogs -Force
                    } else {
                        Set-Content -LiteralPath (Join-Path $boundedLogs ($name + '.omitted')) -Value 'Diagnostic omitted because it exceeded the 262144-byte evidence bound.'
                    }
                }
            }
        }
        # Lifecycle diagnostics live in their own child directory. Preserve the
        # first failing Setup/uninstall log, rather than only its parent's exit.
        $lifecycleLogs = Join-Path $childLogs 'p027'
        if (Test-Path -LiteralPath $lifecycleLogs -PathType Container) {
            $lifecycleDestination = Join-Path $boundedLogs 'p027'
            New-Item $lifecycleDestination -ItemType Directory -Force | Out-Null
            foreach ($item in Get-ChildItem -LiteralPath $lifecycleLogs -Filter '*.log' -File) {
                if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) {
                    if ($item.Length -le 262144) {
                        Copy-Item -LiteralPath $item.FullName -Destination $lifecycleDestination -Force
                    } else {
                        Set-Content -LiteralPath (Join-Path $lifecycleDestination ($item.Name + '.omitted')) -Value 'Diagnostic omitted because it exceeded the 262144-byte evidence bound.'
                    }
                }
            }
        }
    }
    if ($accountCreated) {
        ([ADSI]"WinNT://$env:COMPUTERNAME,computer").Delete('user',$user)
    }
    if ($loadedProfile) {
        Remove-CimInstance -InputObject $loadedProfile -ErrorAction SilentlyContinue
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
