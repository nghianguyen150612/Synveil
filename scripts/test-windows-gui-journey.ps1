[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$ReleaseManifest,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSetupSha256,
    [Parameter(Mandatory=$true)][long]$ExpectedSetupSizeBytes,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedManifestSha256,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][ValidateSet('true','false')][string]$StartupEnabled,
    [Parameter(Mandatory=$true)][ValidateSet('true','false')][string]$DesktopShortcutEnabled,
    [Parameter(Mandatory=$true)][ValidateSet('true','false')][string]$LaunchAfterInstall,
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [int]$TimeoutSeconds = 1200
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
if ($OperatorApproval -cne $approvalText) { throw 'P013_OPERATOR_APPROVAL_REQUIRED' }
if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') {
    throw 'P013_BLOCKED_BY_ENVIRONMENT: hosted Windows service sessions cannot run GUI acceptance'
}
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'P013_REQUIRES_POWERSHELL_7' }
if ($TimeoutSeconds -lt 60 -or $TimeoutSeconds -gt 2400) { throw 'P013_GUI_TIMEOUT_OUT_OF_RANGE' }

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$nativeType = @'
using System;
using System.Runtime.InteropServices;
public static class SynveilP013GuiNative {
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool GetUserObjectInformation(IntPtr handle, int index, System.Text.StringBuilder value, int length, out int needed);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern bool ProcessIdToSessionId(uint processId, out uint sessionId);
  [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
  [DllImport("wtsapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool WTSQuerySessionInformation(IntPtr server, uint sessionId, int infoClass, out IntPtr buffer, out uint bytes);
  [DllImport("wtsapi32.dll")] public static extern void WTSFreeMemory(IntPtr buffer);
}
'@
if (-not ('SynveilP013GuiNative' -as [type])) { Add-Type -TypeDefinition $nativeType }

function Assert-True([bool]$Condition, [string]$Code) {
    if (!$Condition) { throw $Code }
}
function Get-CurrentInteractiveEvidence {
    $session = [uint32]0
    $sessionOk = [SynveilP013GuiNative]::ProcessIdToSessionId([uint32]$PID, [ref]$session)
    if (!$sessionOk -or $session -eq 0) { return $false }
    $desktop = [SynveilP013GuiNative]::OpenInputDesktop(0, $false, 1)
    if ($desktop -eq [IntPtr]::Zero) { return $false }
    $desktopName = [Text.StringBuilder]::new(128)
    $needed = 0
    try {
        if (-not [SynveilP013GuiNative]::GetUserObjectInformation($desktop, 2, $desktopName, $desktopName.Capacity, [ref]$needed) -or
            $desktopName.ToString() -cne 'Default') { return $false }
    } finally { [void][SynveilP013GuiNative]::CloseDesktop($desktop) }
    $buffer = [IntPtr]::Zero
    $bytes = [uint32]0
    if (-not [SynveilP013GuiNative]::WTSQuerySessionInformation([IntPtr]::Zero, $session, 8, [ref]$buffer, [ref]$bytes)) { return $false }
    try { $active = ([Runtime.InteropServices.Marshal]::ReadInt32($buffer) -eq 0) }
    finally { [SynveilP013GuiNative]::WTSFreeMemory($buffer) }
    if (-not $active) { return $false }
    $wtsUser = $null
    $wtsDomain = $null
    foreach ($query in @(@{class=5;name='user'},@{class=7;name='domain'})) {
        $buffer = [IntPtr]::Zero
        $bytes = [uint32]0
        if (-not [SynveilP013GuiNative]::WTSQuerySessionInformation([IntPtr]::Zero, $session, $query.class, [ref]$buffer, [ref]$bytes)) { return $false }
        try { $value = [Runtime.InteropServices.Marshal]::PtrToStringUni($buffer) }
        finally { [SynveilP013GuiNative]::WTSFreeMemory($buffer) }
        if ($query.name -eq 'user') { $wtsUser = $value } else { $wtsDomain = $value }
    }
    if ([string]::IsNullOrWhiteSpace($wtsUser)) { return $false }
    try {
        $qualified = if ([string]::IsNullOrWhiteSpace($wtsDomain)) { $wtsUser } else { "$wtsDomain\$wtsUser" }
        $wtsSid = ([Security.Principal.NTAccount]::new($qualified)).Translate([Security.Principal.SecurityIdentifier]).Value
    } catch { return $false }
    return ($wtsSid -ceq ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value))
}
function Get-ProcessWindow([Diagnostics.Process]$Process) {
    if ($Process.HasExited -or $Process.MainWindowHandle -eq [IntPtr]::Zero) { return $null }
    try { return [System.Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle) }
    catch { return $null }
}
function Get-WindowNames([Diagnostics.Process]$Process) {
    $window = Get-ProcessWindow $Process
    if ($null -eq $window) { return @() }
    $names = [Collections.Generic.List[string]]::new()
    $names.Add([string]$window.Current.Name)
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $Process.Id)
    $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    for ($i = 0; $i -lt $elements.Count -and $i -lt 512; $i++) {
        $name = [string]$elements.Item($i).Current.Name
        if (![string]::IsNullOrWhiteSpace($name)) { $names.Add($name) }
    }
    return @($names)
}
function Wait-ForUiStage([Diagnostics.Process]$Process, [string]$Label, [string]$Pattern, [DateTimeOffset]$Deadline) {
    while ([DateTimeOffset]::UtcNow -lt $Deadline) {
        if ($Process.HasExited) { throw "P013_GUI_SETUP_EXITED_BEFORE_$Label" }
        $names = Get-WindowNames $Process
        if (($names -join "`n") -match $Pattern) {
            Write-Host "UI Automation observed: $Label"
            return $names
        }
        Start-Sleep -Milliseconds 150
        $Process.Refresh()
    }
    throw "P013_GUI_TIMEOUT_$Label"
}
function Get-ToggleValue([Diagnostics.Process]$Process, [string]$NamePattern) {
    $window = Get-ProcessWindow $Process
    if ($null -eq $window) { throw 'P013_GUI_CONTROL_UNAVAILABLE' }
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $Process.Id)
    $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    for ($i = 0; $i -lt $elements.Count -and $i -lt 512; $i++) {
        $element = $elements.Item($i)
        if ([string]$element.Current.Name -match $NamePattern) {
            $pattern = $null
            if ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
                return ([System.Windows.Automation.TogglePattern]$pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
            }
        }
    }
    throw 'P013_GUI_OPTION_CONTROL_NOT_ACCESSIBLE'
}
function Test-WindowValueContains([Diagnostics.Process]$Process, [string]$ExpectedValue) {
    $window = Get-ProcessWindow $Process
    if ($null -eq $window) { return $false }
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $Process.Id)
    $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    for ($i = 0; $i -lt $elements.Count -and $i -lt 512; $i++) {
        $element = $elements.Item($i)
        $pattern = $null
        if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -and
            [string]([System.Windows.Automation.ValuePattern]$pattern).Current.Value -ceq $ExpectedValue) { return $true }
    }
    return $false
}
function Get-UserSidForProcess([Diagnostics.Process]$Process) {
    $owner = Invoke-CimMethod -InputObject (Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)") -MethodName GetOwnerSid -ErrorAction Stop
    if ($owner.ReturnValue -ne 0) { throw 'P013_GUI_PROCESS_IDENTITY_UNAVAILABLE' }
    return [string]$owner.Sid
}

$setupPath = (Resolve-Path -LiteralPath $Setup).Path
$manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest).Path
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$setupHash = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
$setupItem = Get-Item -LiteralPath $setupPath
Assert-True ($manifestHash -ceq $ExpectedManifestSha256) 'P013_CANDIDATE_MANIFEST_MISMATCH'
Assert-True ($setupHash -ceq $ExpectedSetupSha256 -and $setupItem.Length -eq $ExpectedSetupSizeBytes) 'P013_CANDIDATE_IDENTITY_MISMATCH'
Assert-True ((Get-AuthenticodeSignature -FilePath $setupPath).Status -eq 'NotSigned') 'P013_UNSIGNED_FIXTURE_POLICY_MISMATCH'
$release = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$artifact = @($release.artifacts | Where-Object { $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe' })
Assert-True ($release.schema_version -eq 1 -and $release.product -ceq 'Synveil' -and
    $release.source_commit -ceq $ExpectedSourceCommit -and $artifact.Count -eq 1 -and
    $artifact[0].sha256 -ceq $setupHash -and [long]$artifact[0].size_bytes -eq $setupItem.Length) 'P013_CANDIDATE_MANIFEST_BINDING_MISMATCH'
$os = Get-CimInstance Win32_OperatingSystem
Assert-True ($os.Caption -match 'Windows 11' -and [int]$os.BuildNumber -ge 22000) 'P013_UNSUPPORTED_WINDOWS_BUILD'
Assert-True (Get-CurrentInteractiveEvidence) 'P013_BLOCKED_BY_ENVIRONMENT: no interactive Windows desktop'
$testIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$testPrincipal = [Security.Principal.WindowsPrincipal]::new($testIdentity)
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$adminGroupPresent = @($testIdentity.Groups | Where-Object { $_.Value -eq $adminSid.Value }).Count -gt 0
Assert-True (-not $testPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -and -not $adminGroupPresent) 'P013_GUI_REQUIRES_STANDARD_USER'
$expectedSid = $testIdentity.User.Value
$profile = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
$expectedRoot = Join-Path $profile 'ProgramsSynveil'
$desktopPath = Join-Path $expectedRoot 'synveil-desktop.exe'
$deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
$states = [ordered]@{
    candidate_identity = 'PASS'; setup_as_standard_user = 'NOT_RUN'; welcome = 'NOT_RUN'
    user_options = 'NOT_RUN'; install_location = 'NOT_RUN'; install_confirmation = 'NOT_RUN'
    install_progress = 'NOT_RUN'; completion = 'NOT_RUN'; installed_runtime = 'NOT_RUN'
    installed_launch = 'NOT_RUN'; first_run = 'NOT_RUN'; unexpected_uac = 'NOT_RUN'
    shutdown = 'NOT_RUN'
}
$setupProcess = $null
$desktopProcess = $null
$errorCode = $null
$observedChoices = $null
try {
    $setupProcess = Start-Process -FilePath $setupPath -PassThru
    $setupProcess.Refresh()
    $states.setup_as_standard_user = if ((Get-UserSidForProcess $setupProcess) -ceq $expectedSid) { 'PASS' } else { 'FAIL' }
    Assert-True ($states.setup_as_standard_user -eq 'PASS') 'P013_SETUP_PROCESS_USER_MISMATCH'
    $null = Wait-ForUiStage $setupProcess 'Welcome' 'Install Synveil|Welcome to the Synveil Setup Wizard' $deadline
    $states.welcome = 'PASS'
    $licenseNames = Wait-ForUiStage $setupProcess 'Options and license' 'Review the MIT License|StartupRequestedCheckBox|Start Synveil when I sign in' $deadline
    $startup = Get-ToggleValue $setupProcess 'Start Synveil when I sign in|StartupRequestedCheckBox'
    $shortcut = Get-ToggleValue $setupProcess 'Create a desktop shortcut|DesktopIconRequestedCheckBox'
    $launch = Get-ToggleValue $setupProcess 'Open Synveil after installation|LaunchRequestedCheckBox'
    $observedChoices = [ordered]@{startup_enabled=$startup;desktop_shortcut_enabled=$shortcut;launch_after_install=$launch}
    Assert-True ($startup -eq ($StartupEnabled -eq 'true') -and $shortcut -eq ($DesktopShortcutEnabled -eq 'true') -and $launch -eq ($LaunchAfterInstall -eq 'true')) 'P013_GUI_USER_OPTIONS_MISMATCH'
    $states.user_options = 'PASS'

    $destinationNames = Wait-ForUiStage $setupProcess 'Installation location' 'Select Destination Location|Select the folder in which to install Synveil' $deadline
    Assert-True (Test-WindowValueContains $setupProcess $expectedRoot) 'P013_GUI_INSTALL_ROOT_MISMATCH'
    $states.install_location = 'PASS'
    $null = Wait-ForUiStage $setupProcess 'Install confirmation' 'Ready to Install|Ready to begin Installation' $deadline
    $states.install_confirmation = 'PASS'
    $null = Wait-ForUiStage $setupProcess 'Installation progress' 'Installing Synveil|Extracting files' $deadline
    $states.install_progress = 'PASS'
    $null = Wait-ForUiStage $setupProcess 'Completion' 'Completing the Synveil Setup Wizard|Installation Complete' $deadline
    $states.completion = 'PASS'
    while (-not $setupProcess.HasExited -and [DateTimeOffset]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 150
        $setupProcess.Refresh()
    }
    Assert-True ($setupProcess.HasExited -and $setupProcess.ExitCode -eq 0) 'P013_GUI_SETUP_SHUTDOWN_FAILURE'
    Assert-True (Test-Path -LiteralPath $desktopPath -PathType Leaf) 'P013_INSTALLED_DESKTOP_MISSING'
    & (Join-Path $PSScriptRoot 'test-windows-installed-runtime.ps1') -RuntimeRoot $expectedRoot -Manifest (Join-Path $expectedRoot 'SYNVEIL-MANIFEST.txt')
    if ($LASTEXITCODE -ne 0) { throw 'P013_INSTALLED_RUNTIME_VALIDATION_FAILURE' }
    $states.installed_runtime = 'PASS'

    $existingDesktopProcesses = @(Get-Process -Name 'synveil-desktop' -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -ceq $desktopPath } catch { $false }
    })
    Assert-True ($existingDesktopProcesses.Count -le 1) 'P013_UNEXPECTED_DUPLICATE_DESKTOP_PROCESS'
    if ($existingDesktopProcesses.Count -eq 1) {
        $desktopProcess = $existingDesktopProcesses[0]
    } else {
        $desktopProcess = Start-Process -FilePath $desktopPath -PassThru
    }
    $desktopDeadline = [DateTimeOffset]::UtcNow.AddSeconds(60)
    $firstRunObserved = $false
    while ([DateTimeOffset]::UtcNow -lt $desktopDeadline -and -not $desktopProcess.HasExited) {
        $names = Get-WindowNames $desktopProcess
        if (($names -join "`n") -match 'Welcome to Synveil|Host Synveil|Connect to Synveil') { $firstRunObserved = $true; break }
        Start-Sleep -Milliseconds 200
        $desktopProcess.Refresh()
    }
    Assert-True $firstRunObserved 'P013_FIRST_RUN_UI_NOT_OBSERVED'
    Assert-True ((Get-UserSidForProcess $desktopProcess) -ceq $expectedSid) 'P013_INSTALLED_PROCESS_USER_MISMATCH'
    Assert-True ((Resolve-Path -LiteralPath $desktopPath).Path -ceq [IO.Path]::GetFullPath($desktopPath)) 'P013_INSTALLED_RUNTIME_PATH_MISMATCH'
    $duplicateApps = @(Get-Process -Name 'synveil-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $desktopProcess.Id })
    Assert-True ($duplicateApps.Count -eq 0) 'P013_UNEXPECTED_DUPLICATE_DESKTOP_PROCESS'
    $states.installed_launch = 'PASS'; $states.first_run = 'PASS'; $states.unexpected_uac = 'PASS'
    $window = Get-ProcessWindow $desktopProcess
    if ($null -ne $window) {
        $closePattern = $null
        if ($window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$closePattern)) {
            ([System.Windows.Automation.WindowPattern]$closePattern).Close()
        } else { $desktopProcess.CloseMainWindow() | Out-Null }
    } else { $desktopProcess.CloseMainWindow() | Out-Null }
    if (-not $desktopProcess.WaitForExit(15000)) { $desktopProcess.Kill(); throw 'P013_DESKTOP_SHUTDOWN_TIMEOUT' }
    $states.shutdown = 'PASS'
} catch {
    $errorCode = if ($_.Exception.Message -match '^P013_[A-Z0-9_]+') { $Matches[0] } else { 'P013_GUI_ACCEPTANCE_FAILURE' }
    if ($desktopProcess -and -not $desktopProcess.HasExited) {
        try { $desktopProcess.Kill(); $states.shutdown = 'FAIL' } catch { $states.shutdown = 'FAIL' }
    }
    if ($setupProcess -and -not $setupProcess.HasExited) {
        try { $setupProcess.Kill(); $states.completion = 'FAIL' } catch { $states.completion = 'FAIL' }
    }
}

$result = if ($errorCode) { 'FAIL' } elseif (@($states.Values | Where-Object { $_ -ne 'PASS' }).Count -eq 0) { 'PASS' } else { 'INCOMPLETE' }
$harnessHash = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
$evidence = [ordered]@{
    schema_version=1; result=$result; execution_id='p013-ui-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    source_commit=$ExpectedSourceCommit
    candidate=[ordered]@{sha256=$setupHash;size_bytes=$setupItem.Length;release_manifest_sha256=$manifestHash}
    harness_revision_sha256=$harnessHash;windows_edition=[string]$os.Caption
    windows_build=[int]$os.BuildNumber;architecture='x86_64';privilege='standard_user_non_elevated'
    product_state=$states;user_options=$observedChoices;cleanup_status='NOT_RUN';redacted=$true;error_code=$errorCode
    user_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($expectedSid))).ToLowerInvariant()
}
$evidencePath = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-gui-journey-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $evidencePath) -Force | Out-Null
$evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $evidencePath -Encoding utf8
Write-Host "P013 GUI journey: $result"
Write-Host "Evidence: $evidencePath"
if ($result -ne 'PASS') { exit 1 }
