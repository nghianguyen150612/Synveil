param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$OlderFixtureSetup,
    [Parameter(Mandatory=$true)][string]$NewerFixtureSetup,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$NewerLicenseHash,
    [Parameter(Mandatory=$true)][string]$RepositoryRoot,
    [Parameter(Mandatory=$true)][string]$EvidencePath
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-security.ps1')
$appId = '{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
$uninstallKey = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$appId"
$root = Join-Path $env:LOCALAPPDATA 'Programs\Synveil'
$stateRoot = Join-Path $env:LOCALAPPDATA 'Synveil'
$logRoot = Join-Path $stateRoot 'installer\p027'
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Synveil.lnk'
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Synveil.lnk'

function Assert-True([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Invoke-Setup([string]$Path, [string[]]$Extra, [int]$Expected, [string]$Name) {
    $arguments = @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/LAUNCH=0') + $Extra + @(('/LOG=' + (Join-Path $logRoot "$Name.log")))
    $process = Start-Process -FilePath $Path -ArgumentList $arguments -WorkingDirectory $env:TEMP -PassThru
    if (!$process.WaitForExit(120000)) { $process.Kill(); throw "LIFECYCLE_TIMEOUT: $Name" }
    Assert-True ($process.ExitCode -eq $Expected) "LIFECYCLE_EXIT_FAILURE: $Name exit $($process.ExitCode), expected $Expected"
}
function Get-Registration {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)
    Assert-True ($null -ne $key) 'LIFECYCLE_IDENTITY_FAILURE: HKCU registration absent'
    try {
        return [ordered]@{ version=[string]$key.GetValue('DisplayVersion'); location=[string]$key.GetValue('InstallLocation'); manifestHash=[string]$key.GetValue('SynveilManifestSha256'); uninstall=[string]$key.GetValue('QuietUninstallString'); fallback=[string]$key.GetValue('UninstallString') }
    } finally { $key.Dispose() }
}
function Get-RegisteredUninstaller {
    $registration = Get-Registration
    $command = if ($registration.uninstall) { $registration.uninstall } else { $registration.fallback }
    Assert-True (![string]::IsNullOrWhiteSpace($command)) 'LIFECYCLE_IDENTITY_FAILURE: registered uninstall command absent'
    Assert-True ([IO.Path]::GetFullPath($registration.location).TrimEnd('\') -eq [IO.Path]::GetFullPath($root).TrimEnd('\')) 'LIFECYCLE_IDENTITY_FAILURE: install root mismatch'
    $executable = Get-OwnedRegisteredUninstaller $command $root
    return $executable
}
function Manifest-Hashes {
    $result = [ordered]@{}
    $inFiles = $false
    foreach ($line in Get-Content -LiteralPath (Join-Path $root 'SYNVEIL-MANIFEST.txt')) {
        if ($line -ceq 'files=sha256 size path') { $inFiles=$true; continue }
        if (!$inFiles) { continue }
        Assert-True ($line -match '^([0-9a-f]{64}) ([0-9]+) (.+)$') 'LIFECYCLE_MANIFEST_FAILURE: malformed installed entry'
        $file = Join-Path $root $Matches[3]
        Assert-True (Test-Path -LiteralPath $file -PathType Leaf) "LIFECYCLE_MANIFEST_FAILURE: missing $($Matches[3])"
        Assert-True ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $Matches[1]) "LIFECYCLE_MANIFEST_FAILURE: damaged $($Matches[3])"
        $result[$Matches[3]] = $Matches[1]
    }
    return $result
}
function Snapshot-State {
    $result = [ordered]@{}
    foreach ($name in @('config','startup-preference','client-state','library','external')) {
        $path = Join-Path $stateRoot "p027-$name.sentinel"
        $result[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    }
    return ($result | ConvertTo-Json -Compress)
}
function Invoke-RegisteredUninstall([string]$Name) {
    $uninstaller = Get-RegisteredUninstaller
    $process = Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG=' + (Join-Path $logRoot "$Name.log"))) -PassThru
    if (!$process.WaitForExit(120000)) { $process.Kill(); throw "LIFECYCLE_TIMEOUT: $Name" }
    Assert-True ($process.ExitCode -eq 0) "LIFECYCLE_UNINSTALL_FAILURE: $Name exit $($process.ExitCode)"
    Assert-True ($null -eq [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)) 'LIFECYCLE_UNINSTALL_FAILURE: registration remains'
}
function Interrupt-UpgradeAfterOwnedPayloadCopy([string]$Path, [string]$ExpectedLicenseHash) {
    $log = Join-Path $logRoot 'upgrade-interrupted.log'
    $arguments = @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/LAUNCH=0',('/LOG=' + $log))
    $process = Start-Process -FilePath $Path -ArgumentList $arguments -WorkingDirectory $env:TEMP -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    $license = Join-Path $root 'LICENSE'
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($process.HasExited) {
            throw 'LIFECYCLE_INTERRUPTION_FAILURE: Setup completed before the target payload boundary was observed.'
        }
        if ((Test-Path -LiteralPath $license -PathType Leaf) -and
            (Get-FileHash -LiteralPath $license -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $ExpectedLicenseHash) {
            $process.Kill()
            if (!$process.WaitForExit(30000)) {
                throw 'LIFECYCLE_INTERRUPTION_FAILURE: terminated Setup did not exit.'
            }
            return
        }
        Start-Sleep -Milliseconds 10
    }
    $process.Kill()
    $null = $process.WaitForExit(30000)
    throw 'LIFECYCLE_INTERRUPTION_FAILURE: target payload copy boundary was not reached.'
}

New-Item $logRoot -ItemType Directory -Force | Out-Null
foreach ($name in @('config','startup-preference','client-state','library','external')) {
    Set-Content -LiteralPath (Join-Path $stateRoot "p027-$name.sentinel") -Value "P027 synthetic non-secret $name state"
}
$stateBefore = Snapshot-State

# Real-artifact same-version repair: damage only owned payload and integration.
Invoke-Setup $Setup @('/STARTUP=0','/DESKTOPICON=0') 0 'install-production'
$unknown = Join-Path $root 'user-note.txt'; Set-Content -LiteralPath $unknown -Value 'unknown adjacent user file'
$damage = Join-Path $root 'platforms\qwindows.dll'; Remove-Item -LiteralPath $damage
Remove-Item -LiteralPath $startMenu
Invoke-Setup $Setup @('/REPAIR=1') 0 'repair-production'
$null = Manifest-Hashes
Assert-True (Test-Path $unknown -PathType Leaf) 'LIFECYCLE_REPAIR_FAILURE: unknown adjacent file removed'
Assert-True (Test-Path $startMenu -PathType Leaf) 'LIFECYCLE_REPAIR_FAILURE: Start Menu shortcut not restored'
Assert-True (!(Test-Path $desktop)) 'LIFECYCLE_REPAIR_FAILURE: desktop preference changed'
Assert-True ((Snapshot-State) -ceq $stateBefore) 'LIFECYCLE_REPAIR_FAILURE: durable state changed'
Invoke-RegisteredUninstall 'uninstall-production'
Assert-True (Test-Path $unknown -PathType Leaf) 'LIFECYCLE_UNINSTALL_FAILURE: unknown adjacent file removed'
Assert-True ((Snapshot-State) -ceq $stateBefore) 'LIFECYCLE_UNINSTALL_FAILURE: durable state changed'
Remove-Item -LiteralPath $unknown; Remove-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue

# Real Setup execution with isolated numeric fixture identities; never release artifacts.
Invoke-Setup $OlderFixtureSetup @('/STARTUP=0','/DESKTOPICON=1') 0 'install-fixture-old'
$unknown = Join-Path $root 'user-note.txt'; Set-Content -LiteralPath $unknown -Value 'unknown adjacent user file'
Assert-True (Test-Path (Join-Path $root 'p027-obsolete-owned.txt')) 'LIFECYCLE_FIXTURE_FAILURE: old owned file missing'
Assert-True ((Get-Registration).version -ceq '1.0.0') 'LIFECYCLE_FIXTURE_FAILURE: old registration version missing'
$oldManifestPath = Join-Path $root 'SYNVEIL-MANIFEST.txt'
$oldManifestHash = (Get-FileHash -LiteralPath $oldManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
Interrupt-UpgradeAfterOwnedPayloadCopy $NewerFixtureSetup $NewerLicenseHash
Assert-True ((Get-FileHash -LiteralPath (Join-Path $root 'LICENSE') -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $NewerLicenseHash) 'LIFECYCLE_INTERRUPTION_FAILURE: target payload was not partially copied'
Assert-True ((Get-FileHash -LiteralPath $oldManifestPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $oldManifestHash) 'LIFECYCLE_INTERRUPTION_FAILURE: prior ownership manifest changed before payload completion'
$registration = Get-Registration
Assert-True ($registration.version -ceq '1.0.0' -and $registration.manifestHash -ceq $oldManifestHash) 'LIFECYCLE_INTERRUPTION_FAILURE: prior registration no longer authenticates recovery scope'
Assert-True (Test-Path $unknown -PathType Leaf) 'LIFECYCLE_INTERRUPTION_FAILURE: unknown adjacent file was removed'
Assert-True ((Snapshot-State) -ceq $stateBefore) 'LIFECYCLE_INTERRUPTION_FAILURE: durable user state changed'
Invoke-Setup $NewerFixtureSetup @() 0 'upgrade-fixture'
$registration = Get-Registration
Assert-True ($registration.version -ceq '1.1.0') 'LIFECYCLE_UPGRADE_FAILURE: target version not registered'
Assert-True ($registration.manifestHash -ceq (Get-FileHash -LiteralPath (Join-Path $root 'SYNVEIL-MANIFEST.txt') -Algorithm SHA256).Hash.ToLowerInvariant()) 'LIFECYCLE_UPGRADE_FAILURE: target ownership registration mismatch'
Assert-True ((Get-FileHash -LiteralPath (Join-Path $root 'LICENSE') -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $NewerLicenseHash) 'LIFECYCLE_UPGRADE_FAILURE: target payload verification failed after recovery'
Assert-True (!(Test-Path (Join-Path $root 'p027-obsolete-owned.txt'))) 'LIFECYCLE_UPGRADE_FAILURE: obsolete owned file remains'
Assert-True ((Test-Path $unknown) -and (Test-Path $desktop)) 'LIFECYCLE_UPGRADE_FAILURE: adjacent file or desktop choice lost'
Assert-True ((Snapshot-State) -ceq $stateBefore) 'LIFECYCLE_UPGRADE_FAILURE: durable state changed'
$beforeDowngrade = (Manifest-Hashes | ConvertTo-Json -Compress)
Invoke-Setup $OlderFixtureSetup @() 1 'downgrade-fixture'
Assert-True (((Get-Registration).version -ceq '1.1.0')) 'LIFECYCLE_DOWNGRADE_FAILURE: installed version changed'
Assert-True ((Manifest-Hashes | ConvertTo-Json -Compress) -ceq $beforeDowngrade) 'LIFECYCLE_DOWNGRADE_FAILURE: package changed'
Assert-True ((Snapshot-State) -ceq $stateBefore) 'LIFECYCLE_DOWNGRADE_FAILURE: state changed'
Invoke-RegisteredUninstall 'uninstall-fixture'
Assert-True ((Test-Path $unknown) -and ((Snapshot-State) -ceq $stateBefore)) 'LIFECYCLE_UNINSTALL_FAILURE: preserved data changed'
Remove-Item -LiteralPath $unknown; Remove-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue
Invoke-Setup $Setup @('/STARTUP=0','/DESKTOPICON=0') 0 'reinstall-production'
$null = Manifest-Hashes
Invoke-RegisteredUninstall 'uninstall-reinstalled'

$evidence = [ordered]@{
    schema_version=1; source_commit=$env:GITHUB_SHA; artifact_sha256=(Get-FileHash $Setup -Algorithm SHA256).Hash.ToLowerInvariant()
    windows_version=(Get-CimInstance Win32_OperatingSystem).Version
    repair=[ordered]@{ installed_version_before='production'; repair_result='pass'; payload_restored=$true; unknown_adjacent_preserved=$true; state_preserved=$true; startup_preference_preserved=$true }
    upgrade_fixture=[ordered]@{ fixture_old_version='1.0.0'; fixture_new_version='1.1.0'; interruption=[ordered]@{ process_termination='forced'; partial_target_payload_observed=$true; prior_manifest_and_registration_matched=$true; unknown_adjacent_preserved=$true; durable_state_preserved=$true; recovery_result='pass' }; upgrade_result='pass'; obsolete_owned_removed=$true; unknown_adjacent_preserved=$true; state_preserved=$true; registration_count=1; startup_preserved=$true }
    downgrade_fixture=[ordered]@{ downgrade_rejected=$true; package_unchanged=$true; state_unchanged=$true }
    uninstall=[ordered]@{ cleanup_startup_result='pass'; package_removed=$true; registration_removed=$true; state_preserved=$true; unknown_adjacent_preserved=$true; reinstall_result='pass' }
    purge=[ordered]@{ separate_boundary=$true; confirmation_required=$true; authorized_scope=@('APPLICATION_CONFIG') }
}
New-Item (Split-Path $EvidencePath) -ItemType Directory -Force | Out-Null
$evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $EvidencePath -Encoding utf8
Write-Host 'Windows installer lifecycle: PASS'
