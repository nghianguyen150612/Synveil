param(
    [Parameter(Mandatory=$true)][string]$OlderFixtureSetup,
    [Parameter(Mandatory=$true)][string]$NewerFixtureSetup,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$NewerLicenseHash,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$NewerRuntimePayloadHash,
    [Parameter(Mandatory=$true)][string]$EvidencePath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-security.ps1')

$appId = '{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
$uninstallKey = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$appId"
$root = Join-Path $env:LOCALAPPDATA 'Programs\Synveil'
$stateRoot = Join-Path $env:RUNNER_TEMP 'p044-windows-user-state'
$configRoot = Join-Path $stateRoot 'config'
$dataRoot = Join-Path $stateRoot 'data'
$cacheRoot = Join-Path $stateRoot 'cache'
$runtimeRoot = Join-Path $stateRoot 'runtime'
$logRoot = Join-Path $stateRoot 'logs'
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Synveil.lnk'
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Synveil.lnk'
$unknown = Join-Path $root 'user-note.txt'
$manifestPath = Join-Path $root 'SYNVEIL-MANIFEST.txt'
$startupPreference = Join-Path $configRoot 'startup-preference.conf'

function Assert-True([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}

function Get-Registration {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)
    Assert-True ($null -ne $key) 'P044_WINDOWS_IDENTITY_FAILURE: HKCU registration absent.'
    try {
        return [ordered]@{
            version=[string]$key.GetValue('DisplayVersion')
            location=[string]$key.GetValue('InstallLocation')
            manifestHash=[string]$key.GetValue('SynveilManifestSha256')
            uninstall=[string]$key.GetValue('QuietUninstallString')
            fallback=[string]$key.GetValue('UninstallString')
        }
    } finally { $key.Dispose() }
}

function Assert-StartupDisabled {
    Assert-True (Test-Path -LiteralPath $startupPreference -PathType Leaf) 'P044_WINDOWS_PRESERVATION_FAILURE: startup preference was not written.'
    $preference = [IO.File]::ReadAllText($startupPreference)
    Assert-True ($preference -ceq "version=1`nstate=disabled`n") 'P044_WINDOWS_PRESERVATION_FAILURE: disabled startup preference changed.'
}

function Get-StateSnapshot {
    $result = [ordered]@{}
    foreach ($name in @('application-config', 'credentials', 'client-state', 'library', 'server-config', 'server-database', 'server-object-data', 'external')) {
        $parent = if ($name -ceq 'application-config') { $configRoot } else { $dataRoot }
        $path = Join-Path $parent "$name.sentinel"
        Assert-True (Test-Path -LiteralPath $path -PathType Leaf) "P044_WINDOWS_PRESERVATION_FAILURE: missing $name sentinel."
        $result[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $result['startup-preference'] = (Get-FileHash -LiteralPath $startupPreference -Algorithm SHA256).Hash.ToLowerInvariant()
    return ($result | ConvertTo-Json -Compress)
}

function Get-ManifestHashes {
    Assert-True (Test-Path -LiteralPath $manifestPath -PathType Leaf) 'P044_WINDOWS_MANIFEST_FAILURE: package ownership manifest is missing.'
    $result = [ordered]@{}
    $inFiles = $false
    foreach ($line in [IO.File]::ReadAllLines($manifestPath)) {
        if ($line -ceq 'files=sha256 size path') { $inFiles = $true; continue }
        if (!$inFiles) { continue }
        Assert-True ($line -match '^([0-9a-f]{64}) ([0-9]+) (.+)$') 'P044_WINDOWS_MANIFEST_FAILURE: malformed ownership entry.'
        $expectedHash = $Matches[1]
        $expectedSize = [int64]$Matches[2]
        $relative = $Matches[3]
        $path = [IO.Path]::GetFullPath((Join-Path $root $relative))
        $relativeToRoot = [IO.Path]::GetRelativePath([IO.Path]::GetFullPath($root), $path)
        Assert-SafeRelativeIdentity $relativeToRoot
        Assert-True (Test-Path -LiteralPath $path -PathType Leaf) "P044_WINDOWS_MANIFEST_FAILURE: missing $relative."
        Assert-True ((Get-Item -LiteralPath $path).Length -eq $expectedSize) "P044_WINDOWS_MANIFEST_FAILURE: size mismatch for $relative."
        $actualHash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        Assert-True ($actualHash -ceq $expectedHash) "P044_WINDOWS_MANIFEST_FAILURE: digest mismatch for $relative."
        $result[$relative] = $actualHash
    }
    return $result
}

function Invoke-Setup([string]$Path, [string[]]$Extra, [int]$Expected, [string]$Name) {
    $log = Join-Path $logRoot "$Name.log"
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LAUNCH=0') + $Extra + @(('/LOG=' + $log))
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = [IO.Path]::GetFullPath($Path)
    $info.WorkingDirectory = $env:RUNNER_TEMP
    $info.UseShellExecute = $false
    foreach ($argument in $arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    try {
        if (!$process.WaitForExit(120000)) {
            $process.Kill($true)
            $process.WaitForExit()
            throw "P044_WINDOWS_TIMEOUT: $Name"
        }
        Assert-True ($process.ExitCode -eq $Expected) "P044_WINDOWS_SETUP_FAILURE: $Name exit $($process.ExitCode), expected $Expected."
    } finally { $process.Dispose() }
}

function Interrupt-SetupAfterLicenseCopy([string]$Path, [string[]]$Extra, [string]$Name) {
    $log = Join-Path $logRoot "$Name.log"
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LAUNCH=0') + $Extra + @(('/LOG=' + $log))
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = [IO.Path]::GetFullPath($Path)
    $info.WorkingDirectory = $env:RUNNER_TEMP
    $info.UseShellExecute = $false
    foreach ($argument in $arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    $terminated = $false
    try {
        while ([DateTime]::UtcNow -lt $deadline) {
            $process.Refresh()
            if ($process.HasExited) {
                throw 'P044_WINDOWS_INTERRUPTION_FAILURE: Setup completed before the changed payload boundary was observed.'
            }
            if ((Test-Path -LiteralPath (Join-Path $root 'LICENSE') -PathType Leaf) -and
                (Get-FileHash -LiteralPath (Join-Path $root 'LICENSE') -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $NewerLicenseHash) {
                $process.Refresh()
                if ($process.HasExited) {
                    throw 'P044_WINDOWS_INTERRUPTION_FAILURE: Setup exited before forced termination.'
                }
                $process.Kill($true)
                if (!$process.WaitForExit(30000)) {
                    throw 'P044_WINDOWS_INTERRUPTION_FAILURE: terminated Setup did not exit.'
                }
                $terminated = $true
                break
            }
            Start-Sleep -Milliseconds 5
        }
        Assert-True $terminated 'P044_WINDOWS_INTERRUPTION_FAILURE: changed target LICENSE was not observed before timeout.'
    } finally {
        if (!$process.HasExited) { $process.Kill($true); $process.WaitForExit(30000) | Out-Null }
        $process.Dispose()
    }
}

foreach ($setupPath in @($OlderFixtureSetup, $NewerFixtureSetup)) {
    Assert-True (Test-Path -LiteralPath $setupPath -PathType Leaf) 'P044_WINDOWS_FIXTURE_FAILURE: compiled Setup input is missing.'
    Assert-NoReparseAncestry $setupPath $true
}
Assert-True (!(Test-Path -LiteralPath $root)) 'P044_WINDOWS_FIXTURE_FAILURE: disposable runner already has a Synveil install path.'
Assert-True ($null -eq [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)) 'P044_WINDOWS_FIXTURE_FAILURE: disposable runner already has Synveil registration.'

New-Item -ItemType Directory -Path $configRoot, $dataRoot, $cacheRoot, $runtimeRoot, $logRoot -Force | Out-Null
$env:SYNVEIL_CONFIG_DIR = $configRoot
$env:SYNVEIL_DATA_DIR = $dataRoot
$env:SYNVEIL_CACHE_DIR = $cacheRoot
$env:SYNVEIL_RUNTIME_DIR = $runtimeRoot
foreach ($name in @('application-config', 'credentials', 'client-state', 'library', 'server-config', 'server-database', 'server-object-data', 'external')) {
    $parent = if ($name -ceq 'application-config') { $configRoot } else { $dataRoot }
    [IO.File]::WriteAllText((Join-Path $parent "$name.sentinel"), "P044 synthetic preserved $name data`n", [Text.UTF8Encoding]::new($false))
}

Invoke-Setup $OlderFixtureSetup @('/STARTUP=0', '/DESKTOPICON=1') 0 'install-old'
Assert-True ((Get-Registration).version -ceq '1.0.0') 'P044_WINDOWS_FIXTURE_FAILURE: prior version is not registered.'
Assert-StartupDisabled
$stateBefore = Get-StateSnapshot
$oldManifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText($unknown, 'P044 unknown adjacent user file', [Text.UTF8Encoding]::new($false))
$oldOwned = Join-Path $root 'p044-obsolete-owned.txt'
Assert-True (Test-Path -LiteralPath $oldOwned -PathType Leaf) 'P044_WINDOWS_FIXTURE_FAILURE: old owned file is absent.'

Interrupt-SetupAfterLicenseCopy $NewerFixtureSetup @() 'upgrade-interrupted'
Assert-True ((Get-FileHash -LiteralPath (Join-Path $root 'LICENSE') -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $NewerLicenseHash) 'P044_WINDOWS_INTERRUPTION_FAILURE: target LICENSE was not copied.'
Assert-True ((Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $oldManifestHash) 'P044_WINDOWS_INTERRUPTION_FAILURE: prior ownership manifest changed during partial copy.'
$registration = Get-Registration
Assert-True ($registration.version -ceq '1.0.0' -and $registration.manifestHash -ceq $oldManifestHash) 'P044_WINDOWS_INTERRUPTION_FAILURE: prior registration no longer authenticates recovery scope.'
$interruptedRuntimeHash = (Get-FileHash -LiteralPath (Join-Path $root 'runtime-payload.bin') -Algorithm SHA256).Hash.ToLowerInvariant()
Assert-True ($interruptedRuntimeHash -cne $NewerRuntimePayloadHash) 'P044_WINDOWS_INTERRUPTION_FAILURE: fixture did not stop before the later owned payload boundary.'
Assert-True (Test-Path -LiteralPath $unknown -PathType Leaf) 'P044_WINDOWS_INTERRUPTION_FAILURE: unknown adjacent file was removed.'
Assert-True ((Get-StateSnapshot) -ceq $stateBefore) 'P044_WINDOWS_INTERRUPTION_FAILURE: durable user-state sentinels changed.'
Assert-StartupDisabled

# A new Setup process must derive recovery from the old registered manifest,
# finish the supported forward upgrade, and verify the complete target closure.
Invoke-Setup $NewerFixtureSetup @() 0 'recover-forward-upgrade'
$registration = Get-Registration
$newManifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
Assert-True ($registration.version -ceq '1.1.0' -and $registration.manifestHash -ceq $newManifestHash) 'P044_WINDOWS_RECOVERY_FAILURE: target registration does not match the new manifest.'
$verifiedPayload = Get-ManifestHashes
Assert-True ($verifiedPayload['LICENSE'] -ceq $NewerLicenseHash) 'P044_WINDOWS_RECOVERY_FAILURE: target license verification failed.'
Assert-True ($verifiedPayload['runtime-payload.bin'] -ceq $NewerRuntimePayloadHash) 'P044_WINDOWS_RECOVERY_FAILURE: target payload verification failed.'
Assert-True (!(Test-Path -LiteralPath $oldOwned)) 'P044_WINDOWS_RECOVERY_FAILURE: trusted obsolete owned file remains.'
Assert-True (Test-Path -LiteralPath $unknown -PathType Leaf) 'P044_WINDOWS_RECOVERY_FAILURE: unknown adjacent file was removed.'
Assert-True ((Test-Path -LiteralPath $startMenu -PathType Leaf) -and (Test-Path -LiteralPath $desktop -PathType Leaf)) 'P044_WINDOWS_RECOVERY_FAILURE: explicit shortcut preference was not preserved.'
Assert-True ((Get-StateSnapshot) -ceq $stateBefore) 'P044_WINDOWS_RECOVERY_FAILURE: durable user-state sentinels changed.'
Assert-StartupDisabled
$verifiedBeforeDowngrade = $verifiedPayload | ConvertTo-Json -Compress

# Interrupted recovery does not make downgrade safe. Rejected Setup must leave
# the verified target payload, user choice, and adjacent files untouched.
Invoke-Setup $OlderFixtureSetup @() 1 'reject-downgrade'
Assert-True (((Get-Registration).version) -ceq '1.1.0') 'P044_WINDOWS_DOWNGRADE_FAILURE: installed version changed.'
Assert-True (((Get-ManifestHashes | ConvertTo-Json -Compress)) -ceq $verifiedBeforeDowngrade) 'P044_WINDOWS_DOWNGRADE_FAILURE: installed payload changed.'
Assert-True ((Get-StateSnapshot) -ceq $stateBefore) 'P044_WINDOWS_DOWNGRADE_FAILURE: durable user-state sentinels changed.'
Assert-StartupDisabled

# Damage only manifest-owned files, then interrupt real Inno during same-version
# repair. The trusted manifest and registration remain the scope authority.
$repairManifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$repairRegistration = Get-Registration
Assert-True ($repairRegistration.version -ceq '1.1.0' -and $repairRegistration.manifestHash -ceq $repairManifestHash) 'P044_WINDOWS_REPAIR_FAILURE: trusted repair identity is not established.'
$repairLicense = Join-Path $root 'LICENSE'
$repairPayload = Join-Path $root 'runtime-payload.bin'
[IO.File]::WriteAllText($repairLicense, 'P044 damaged owned license fixture', [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($repairPayload, 'P044 damaged owned large payload fixture', [Text.UTF8Encoding]::new($false))
$damagedOwned = Join-Path $root 'platforms\qwindows.dll'
Remove-Item -LiteralPath $damagedOwned -Force
Remove-Item -LiteralPath $startMenu -Force

Interrupt-SetupAfterLicenseCopy $NewerFixtureSetup @('/REPAIR=1') 'repair-interrupted'
Assert-True ((Get-FileHash -LiteralPath $repairLicense -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $NewerLicenseHash) 'P044_WINDOWS_REPAIR_INTERRUPTION_FAILURE: owned license was not restored before termination.'
Assert-True ((Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $repairManifestHash) 'P044_WINDOWS_REPAIR_INTERRUPTION_FAILURE: trusted manifest changed.'
$registration = Get-Registration
Assert-True ($registration.version -ceq '1.1.0' -and $registration.manifestHash -ceq $repairManifestHash) 'P044_WINDOWS_REPAIR_INTERRUPTION_FAILURE: trusted registration changed.'
$interruptedRepairPayloadHash = (Get-FileHash -LiteralPath $repairPayload -Algorithm SHA256).Hash.ToLowerInvariant()
Assert-True ($interruptedRepairPayloadHash -cne $NewerRuntimePayloadHash) 'P044_WINDOWS_REPAIR_INTERRUPTION_FAILURE: fixture did not stop before full owned payload restoration.'
Assert-True (Test-Path -LiteralPath $unknown -PathType Leaf) 'P044_WINDOWS_REPAIR_INTERRUPTION_FAILURE: unknown adjacent file was removed.'
Assert-True ((Get-StateSnapshot) -ceq $stateBefore) 'P044_WINDOWS_REPAIR_INTERRUPTION_FAILURE: durable user-state sentinels changed.'
Assert-StartupDisabled

# A fresh same-version Setup process completes the scoped repair and verifies
# every target-owned file before treating the installation as ready.
Invoke-Setup $NewerFixtureSetup @('/REPAIR=1') 0 'same-version-repair'
$null = Get-ManifestHashes
Assert-True (Test-Path -LiteralPath $unknown -PathType Leaf) 'P044_WINDOWS_REPAIR_FAILURE: unknown adjacent file was removed.'
Assert-True (Test-Path -LiteralPath $startMenu -PathType Leaf) 'P044_WINDOWS_REPAIR_FAILURE: owned shortcut was not restored.'
Assert-True (!(Test-Path -LiteralPath $desktop)) 'P044_WINDOWS_REPAIR_FAILURE: explicit desktop shortcut choice changed.'
Assert-True ((Get-StateSnapshot) -ceq $stateBefore) 'P044_WINDOWS_REPAIR_FAILURE: durable user-state sentinels changed.'
Assert-StartupDisabled

$registration = Get-Registration
$uninstaller = if ($registration.uninstall) { $registration.uninstall } else { $registration.fallback }
$uninstallerPath = Get-OwnedRegisteredUninstaller $uninstaller $root
$uninstallInfo = [Diagnostics.ProcessStartInfo]::new()
$uninstallInfo.FileName = $uninstallerPath
$uninstallInfo.WorkingDirectory = $env:RUNNER_TEMP
$uninstallInfo.UseShellExecute = $false
foreach ($argument in @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', ('/LOG=' + (Join-Path $logRoot 'uninstall.log')))) { $uninstallInfo.ArgumentList.Add($argument) }
$uninstallProcess = [Diagnostics.Process]::Start($uninstallInfo)
try {
    Assert-True ($uninstallProcess.WaitForExit(120000)) 'P044_WINDOWS_UNINSTALL_FAILURE: uninstaller exceeded its bound.'
    Assert-True ($uninstallProcess.ExitCode -eq 0) 'P044_WINDOWS_UNINSTALL_FAILURE: ordinary uninstall did not complete.'
} finally { $uninstallProcess.Dispose() }
Assert-True ($null -eq [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)) 'P044_WINDOWS_UNINSTALL_FAILURE: registration remains.'
Assert-True (Test-Path -LiteralPath $unknown -PathType Leaf) 'P044_WINDOWS_UNINSTALL_FAILURE: unknown adjacent file was removed.'
Assert-True ((Get-StateSnapshot) -ceq $stateBefore) 'P044_WINDOWS_UNINSTALL_FAILURE: durable user-state sentinels changed.'
Assert-StartupDisabled

$evidence = [ordered]@{
    schema_version=1
    evidence_class='native CI process-interruption'
    power_loss='BLOCKED / unavailable native VM power-cycle evidence'
    state_scope='test-owned RUNNER_TEMP sentinels; no pre-existing user or server data was accessed'
    source_commit=$env:GITHUB_SHA
    windows_version=(Get-CimInstance Win32_OperatingSystem).Version
    installer=[ordered]@{
        implementation='repository Synveil.iss compiled by the locked Inno Setup toolchain'
        inno_version='6.7.3'
        older_setup_sha256=(Get-FileHash -LiteralPath $OlderFixtureSetup -Algorithm SHA256).Hash.ToLowerInvariant()
        newer_setup_sha256=(Get-FileHash -LiteralPath $NewerFixtureSetup -Algorithm SHA256).Hash.ToLowerInvariant()
        payload_kind='bounded synthetic package manifest; real release synveil-client.exe; synthetic Qt/runtime placeholders; desktop executable is not launched'
    }
    interruption=[ordered]@{
        process_termination='forced process-tree termination after target LICENSE copy and before later runtime-payload.bin reached target hash'
        observed_prior_manifest_and_registration=$true
        unknown_adjacent_file_preserved=$true
        test_owned_application_and_server_state_sentinels_preserved=$true
        disabled_startup_preference_preserved=$true
        recovery='new compatible Setup process; complete target ownership manifest, package hashes, registration, shortcut preference and preservation verified'
    }
    forward_upgrade=[ordered]@{ target_version='1.1.0'; obsolete_owned_file_removed=$true; unknown_adjacent_file_preserved=$true }
    downgrade=[ordered]@{ older_setup_rejected=$true; installed_manifest_and_state_unchanged=$true }
    same_version_repair=[ordered]@{ process_termination='forced during owned payload restoration'; prior_manifest_and_registration_retained=$true; fresh_setup_recovery_verified=$true; owned_payload_and_shortcut_restored=$true; unknown_adjacent_file_preserved=$true; durable_state_preserved=$true }
    ordinary_uninstall=[ordered]@{ package_registration_removed=$true; unknown_adjacent_file_preserved=$true; durable_state_and_startup_preference_preserved=$true }
}
New-Item -ItemType Directory -Path (Split-Path -Parent $EvidencePath) -Force | Out-Null
[IO.File]::WriteAllText($EvidencePath, ($evidence | ConvertTo-Json -Depth 6) + "`n", [Text.UTF8Encoding]::new($false))
Write-Host 'P044 real Inno Setup synthetic-payload process-interruption fixture: PASS'
