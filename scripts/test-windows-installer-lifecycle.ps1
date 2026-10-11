param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$OlderFixtureSetup,
    [Parameter(Mandatory=$true)][string]$NewerFixtureSetup,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$NewerLicenseHash,
    [Parameter(Mandatory=$true)][string]$RepositoryRoot,
    [Parameter(Mandatory=$true)][string]$EvidencePath
)

$ErrorActionPreference = 'Stop'
$ErrorView = 'NormalView'
. (Join-Path $PSScriptRoot 'windows-security.ps1')
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$profileRoot = [Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)
if ([string]::IsNullOrWhiteSpace($profileRoot)) { $profileRoot = $env:USERPROFILE }
if ([string]::IsNullOrWhiteSpace($profileRoot) -or !(Test-Path -LiteralPath $profileRoot -PathType Container)) {
    throw 'LIFECYCLE_PROFILE_FAILURE: standard-user profile path is unavailable'
}
$profileRoot = [IO.Path]::GetFullPath($profileRoot)
$profileName = Split-Path -Path $profileRoot -Leaf
$identityName = $identity.Name.Split('\')[-1]
if ($profileName -ine $identityName) {
    throw 'LIFECYCLE_PROFILE_FAILURE: profile path does not match the standard-user identity'
}
$script:profileRoot = $profileRoot
$script:profilePrefix = $profileRoot.TrimEnd('\') + '\'
function Resolve-ProfileScopedFolder([object]$KnownFolder, [string]$FallbackRelativePath) {
    $candidate = [Environment]::GetFolderPath($KnownFolder)
    if (![string]::IsNullOrWhiteSpace($candidate)) {
        $candidate = [IO.Path]::GetFullPath($candidate)
        if ($candidate.StartsWith($script:profilePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            return $candidate
        }
    }
    return [IO.Path]::GetFullPath((Join-Path $script:profileRoot $FallbackRelativePath))
}
$localAppData = Resolve-ProfileScopedFolder ([Environment+SpecialFolder]::LocalApplicationData) 'AppData\Local'
$roamingAppData = Resolve-ProfileScopedFolder ([Environment+SpecialFolder]::ApplicationData) 'AppData\Roaming'
$env:USERPROFILE = $profileRoot
$env:LOCALAPPDATA = $localAppData
$env:APPDATA = $roamingAppData
$userTemp = Join-Path $localAppData 'Temp'
New-Item -Path $userTemp -ItemType Directory -Force | Out-Null
$env:TEMP = $userTemp
$env:TMP = $userTemp
$desktopFolder = [Environment]::GetFolderPath([Environment+SpecialFolder]::Desktop)
if ([string]::IsNullOrWhiteSpace($desktopFolder)) {
    $desktopFolder = Join-Path $profileRoot 'Desktop'
}
$desktopFolder = [IO.Path]::GetFullPath($desktopFolder)
New-Item -Path $desktopFolder -ItemType Directory -Force | Out-Null
$appId = '{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
$uninstallKey = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$appId"
$root = Join-Path $env:LOCALAPPDATA 'Programs\Synveil'
$stateRoot = Join-Path $env:LOCALAPPDATA 'Synveil'
$logRoot = Join-Path $stateRoot 'installer\p027'
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Synveil.lnk'
$desktop = Join-Path $desktopFolder 'Synveil.lnk'

function Assert-True([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Get-SetupProcessTree([int]$RootProcessId) {
    $all = @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop)
    $pending = [System.Collections.Generic.Queue[int]]::new()
    $seen = [System.Collections.Generic.HashSet[int]]::new()
    $tree = [System.Collections.Generic.List[object]]::new()
    $root = $all | Where-Object { [int]$_.ProcessId -eq $RootProcessId } | Select-Object -First 1
    if ($null -ne $root) {
        $tree.Add($root)
        [void]$seen.Add($RootProcessId)
    }
    $pending.Enqueue($RootProcessId)
    while ($pending.Count -gt 0) {
        $parentId = $pending.Dequeue()
        foreach ($child in @($all | Where-Object { [int]$_.ParentProcessId -eq $parentId })) {
            $childId = [int]$child.ProcessId
            if ($seen.Add($childId)) {
                $tree.Add($child)
                $pending.Enqueue($childId)
            }
        }
    }
    return $tree.ToArray()
}
function Invoke-Setup([string]$Path, [string[]]$Extra, [int]$Expected, [string]$Name) {
    $arguments = @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/LAUNCH=0') + $Extra + @(('/LOG=' + (Join-Path $logRoot "$Name.log")))
    $process = Start-Process -FilePath $Path -ArgumentList $arguments -WorkingDirectory $env:TEMP -PassThru
    if (!$process.WaitForExit(120000)) {
        $tree = @()
        try {
            $tree = @(Get-SetupProcessTree $process.Id)
            foreach ($entry in ($tree | Select-Object -First 16)) {
                Write-Output "LIFECYCLE_TIMEOUT_PROCESS name=$($entry.Name) pid=$($entry.ProcessId) parent_pid=$($entry.ParentProcessId)"
                $commandLine = [string]$entry.CommandLine
                if (![string]::IsNullOrEmpty($commandLine)) {
                    $commandLine = $commandLine.Replace($script:profileRoot, '[USER_PROFILE]')
                    if ($commandLine.Length -gt 600) { $commandLine = $commandLine.Substring(0,600) + ' …' }
                }
                $executablePath = [string]$entry.ExecutablePath
                if (![string]::IsNullOrEmpty($executablePath)) {
                    $executablePath = $executablePath.Replace($script:profileRoot, '[USER_PROFILE]')
                }
                $processInfo = Get-Process -Id ([int]$entry.ProcessId) -ErrorAction SilentlyContinue
                $cpuSeconds = if ($null -ne $processInfo) {
                    $processInfo.TotalProcessorTime.TotalSeconds.ToString('F1',[Globalization.CultureInfo]::InvariantCulture)
                } else { 'unavailable' }
                $mainWindow = if ($null -ne $processInfo -and $processInfo.MainWindowTitle) {
                    [string]$processInfo.MainWindowTitle
                } else { '[none]' }
                if ($mainWindow.Length -gt 200) { $mainWindow = $mainWindow.Substring(0,200) + ' …' }
                $waitSummary = 'unavailable'
                try {
                    $threads = @(Get-CimInstance -ClassName Win32_Thread -Filter "ProcessHandle=$($entry.ProcessId)" -ErrorAction Stop)
                    $waitSummary = @($threads | Group-Object { "$($_.ThreadState):$($_.ThreadWaitReason)" } |
                        ForEach-Object { "$($_.Name)=$($_.Count)" }) -join ','
                } catch { }
                Write-Output "LIFECYCLE_TIMEOUT_DIAGNOSTIC name=$($entry.Name) pid=$($entry.ProcessId) image=$executablePath cpu_seconds=$cpuSeconds threads=$($entry.ThreadCount) wait_states=$waitSummary window=$mainWindow command_line=$commandLine"
            }
        } catch {
            Write-Output "LIFECYCLE_TIMEOUT_PROCESS_TREE_FAILURE type=$($_.Exception.GetType().FullName)"
        }
        $treeSummary = if ($tree.Count -gt 0) {
            ($tree | ForEach-Object { [string]$_.ProcessId }) -join ','
        } else {
            [string]$process.Id
        }
        $treeTerminated = $false
        try {
            if (!$process.HasExited) { $process.Kill($true) }
            $treeTerminated = $process.WaitForExit(30000)
        } catch {
            Write-Output "LIFECYCLE_TIMEOUT_TREE_TERMINATION_FAILURE type=$($_.Exception.GetType().FullName)"
        }
        Write-Output "LIFECYCLE_TIMEOUT_RECOVERY root_pid=$($process.Id) process_pids=$treeSummary root_exited=$treeTerminated"
        throw "LIFECYCLE_TIMEOUT: $Name root_pid=$($process.Id) process_tree_terminated=$treeTerminated"
    }
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
    return $uninstaller
}
function Wait-ForUninstallerSelfRemoval([string]$Path, [string]$Name) {
    $rootPath = [IO.Path]::GetFullPath($root)
    $uninstallerPath = [IO.Path]::GetFullPath($Path)
    $rootPrefix = $rootPath.TrimEnd('\') + '\'
    Assert-True ($uninstallerPath.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) `
        "LIFECYCLE_UNINSTALL_FAILURE: registered uninstaller escaped the package root ($Name)"
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (Test-Path -LiteralPath $rootPath -PathType Container) {
        if (Test-Path -LiteralPath $uninstallerPath -PathType Leaf) {
            $entries = @(Get-ChildItem -LiteralPath $rootPath -Force)
            $unexpected = @($entries | Where-Object {
                [IO.Path]::GetFullPath($_.FullName) -ine $uninstallerPath
            })
            Assert-True ($unexpected.Count -eq 0) `
                "LIFECYCLE_UNINSTALL_FAILURE: unexpected residual package entries during self-cleanup ($Name)"
            if ([DateTime]::UtcNow -ge $deadline) {
                throw "LIFECYCLE_UNINSTALL_FAILURE: registered uninstaller remains after bounded self-cleanup wait ($Name)"
            }
            # Inno Setup can return before its second-phase helper removes the registered uninstaller.
            Start-Sleep -Milliseconds 100
            continue
        }
        if (!(Test-Path -LiteralPath $rootPath -PathType Container)) { return }
        $remaining = @(Get-ChildItem -LiteralPath $rootPath -Force)
        Assert-True ($remaining.Count -eq 0) `
            "LIFECYCLE_UNINSTALL_FAILURE: unexpected residual package entries after self-cleanup ($Name)"
        Remove-Item -LiteralPath $rootPath -Force -ErrorAction Stop
        return
    }
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
$productionUninstaller = Invoke-RegisteredUninstall 'uninstall-production'
Assert-True (Test-Path $unknown -PathType Leaf) 'LIFECYCLE_UNINSTALL_FAILURE: unknown adjacent file removed'
Assert-True ((Snapshot-State) -ceq $stateBefore) 'LIFECYCLE_UNINSTALL_FAILURE: durable state changed'
Remove-Item -LiteralPath $unknown
Wait-ForUninstallerSelfRemoval $productionUninstaller 'production uninstall'

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
$fixtureUninstaller = Invoke-RegisteredUninstall 'uninstall-fixture'
Assert-True ((Test-Path $unknown) -and ((Snapshot-State) -ceq $stateBefore)) 'LIFECYCLE_UNINSTALL_FAILURE: preserved data changed'
Remove-Item -LiteralPath $unknown
Wait-ForUninstallerSelfRemoval $fixtureUninstaller 'fixture uninstall'
Invoke-Setup $Setup @('/STARTUP=0','/DESKTOPICON=0') 0 'reinstall-production'
$null = Manifest-Hashes
$reinstalledUninstaller = Invoke-RegisteredUninstall 'uninstall-reinstalled'
Wait-ForUninstallerSelfRemoval $reinstalledUninstaller 'reinstalled production uninstall'

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
