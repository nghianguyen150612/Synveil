[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$ReleaseManifest,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedHarnessCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSetupSha256,
    [Parameter(Mandatory=$true)][long]$ExpectedSetupSizeBytes,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedManifestSha256,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][ValidateSet('true','false')][string]$StartupEnabled,
    [Parameter(Mandatory=$true)][ValidateSet('true','false')][string]$DesktopShortcutEnabled,
    [Parameter(Mandatory=$true)][ValidateSet('true','false')][string]$LaunchAfterInstall,
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [ValidatePattern('^synveil_p013_[0-9a-f]{8}$')][string]$PeerUserName,
    [int]$TimeoutSeconds = 1200
)

$ErrorActionPreference = 'Stop'
$script:observedSessionId = [uint32]0
$script:observedArchitecture = 'unknown'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
if ($OperatorApproval -cne $approvalText) { throw 'P013_OPERATOR_APPROVAL_REQUIRED' }
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
  [DllImport("advapi32.dll", SetLastError=true)] public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] public static extern bool GetTokenInformation(IntPtr token, int infoClass, out int value, int length, out int needed);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(uint access, bool inherit, uint processId);
  [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
  public static bool IsElevated() {
    IntPtr token; int value, needed;
    if (!OpenProcessToken(GetCurrentProcess(), 0x0008, out token)) throw new System.ComponentModel.Win32Exception();
    try { if (!GetTokenInformation(token, 20, out value, sizeof(int), out needed)) throw new System.ComponentModel.Win32Exception(); return value != 0; }
    finally { CloseHandle(token); }
  }
  public static bool IsProcessElevated(uint processId) {
    IntPtr process = OpenProcess(0x1000, false, processId);
    if (process == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
    try {
      IntPtr token; int value, needed;
      if (!OpenProcessToken(process, 0x0008, out token)) throw new System.ComponentModel.Win32Exception();
      try { if (!GetTokenInformation(token, 20, out value, sizeof(int), out needed)) throw new System.ComponentModel.Win32Exception(); return value != 0; }
      finally { CloseHandle(token); }
    } finally { CloseHandle(process); }
  }
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
    $script:observedSessionId = [uint32]$session
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
function Write-BlockedGuiEvidence([string]$ReasonCode, $OperatingSystem, $SetupItem) {
    $repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
    $harnessFiles = @(
        @{name='scripts/test-windows-gui-journey.ps1';path=$PSCommandPath},
        @{name='crates/client/examples/p013_windows_control_probe.rs';path=(Join-Path $repoRoot 'crates/client/examples/p013_windows_control_probe.rs')}
    )
    $binding = ($harnessFiles | Sort-Object name | ForEach-Object {
        $_.name + ':' + (Get-FileHash -LiteralPath $_.path -Algorithm SHA256).Hash.ToLowerInvariant()
    }) -join "`n"
    $harnessHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($binding))).ToLowerInvariant()
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $elevated = [SynveilP013GuiNative]::IsElevated()
    $states = [ordered]@{}
    foreach ($name in @('setup_as_standard_user','welcome','user_options','install_location','install_confirmation',
        'install_progress','completion','installed_runtime','installed_launch','first_run','unexpected_uac',
        'disposable_test_user','installed_client_process','installed_desktop_ipc_ping','pipe_protocol','pipe_wrong_user',
        'pipe_unauthorized_operation','pipe_stale_profile','pipe_server_termination','pipe_expired_endpoint',
        'pipe_restart','no_tcp_fallback','shutdown','ipc_harness_cleanup','setup_non_elevated',
        'installed_desktop_non_elevated','installed_client_non_elevated','uninstall_preservation')) {
        $states[$name] = 'BLOCKED_BY_ENVIRONMENT'
    }
    $record = [ordered]@{
        schema_version=1;result='BLOCKED_BY_ENVIRONMENT';execution_id='p013-ui-' + [Guid]::NewGuid().ToString('N')
        executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ');source_commit=$ExpectedSourceCommit
        harness_source_commit=$ExpectedHarnessCommit
        candidate=[ordered]@{sha256=$script:setupHash;size_bytes=$SetupItem.Length;release_manifest_sha256=$script:manifestHash}
        harness_revision_sha256=$harnessHash;windows_edition=[string]$OperatingSystem.Caption
        windows_build=[int]$OperatingSystem.BuildNumber;architecture=$script:observedArchitecture
        privilege=if($elevated){'administrator'}else{'standard_user_non_elevated'};token_elevated=[bool]$elevated
        process_session_id=[uint32]$script:observedSessionId;product_state=$states
        ui_evidence=[ordered]@{mode='unavailable';state_assertions=$false;screenshots_status='BLOCKED_BY_ENVIRONMENT'}
        user_options=$null;control_ipc=$null;cleanup_status='NOT_RUN';redacted=$true;error_code=$ReasonCode
        user_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
    }
    $output = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-gui-journey-v1.json'
    New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
    $record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $output -Encoding utf8
    Write-Host "P013 GUI journey: BLOCKED_BY_ENVIRONMENT ($ReasonCode)"
    Write-Host "Evidence: $output"
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
        $identityText = [string]$element.Current.Name + ' ' + [string]$element.Current.AutomationId
        if ($identityText -match $NamePattern) {
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
        if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $actualValue = [string]([System.Windows.Automation.ValuePattern]$pattern).Current.Value
            if ($actualValue.TrimEnd('\') -ieq $ExpectedValue.TrimEnd('\')) { return $true }
        }
    }
    return $false
}
function Get-UserSidForProcess([Diagnostics.Process]$Process) {
    $owner = Invoke-CimMethod -InputObject (Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)") -MethodName GetOwnerSid -ErrorAction Stop
    if ($owner.ReturnValue -ne 0) { throw 'P013_GUI_PROCESS_IDENTITY_UNAVAILABLE' }
    return [string]$owner.Sid
}
function Get-HklmRegistrationCount([string]$Subkey) {
    $count = 0
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,$view)
        try {
            $key = $base.OpenSubKey($Subkey)
            if ($null -ne $key) { $count++; $key.Dispose() }
        } finally { $base.Dispose() }
    }
    return $count
}
function Get-CurrentProfileId([string]$ExpectedSid) {
    $config = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) 'Synveil\client.conf'
    if (-not (Test-Path -LiteralPath $config -PathType Leaf)) { throw 'P013_PROFILE_IDENTITY_UNAVAILABLE' }
    $owner = (Get-Acl -LiteralPath $config).Owner
    try { $ownerSid = ([Security.Principal.NTAccount]::new($owner)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { throw 'P013_PROFILE_OWNER_UNVERIFIABLE' }
    if ($ownerSid -cne $ExpectedSid) { throw 'P013_PROFILE_OWNER_MISMATCH' }
    $lines = [IO.File]::ReadAllLines($config) | Where-Object { $_ -match '^\s*profile_id\s*=' }
    if (@($lines).Count -ne 1 -or $lines[0] -notmatch '^\s*profile_id\s*=\s*([0-9a-f-]{36})\s*$') {
        throw 'P013_PROFILE_IDENTITY_UNAVAILABLE'
    }
    $profileId = $Matches[1].ToLowerInvariant()
    if ($profileId -notmatch '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') {
        throw 'P013_PROFILE_IDENTITY_UNAVAILABLE'
    }
    return $profileId
}
$script:ipcTempRoot = $null
$script:ipcProbeExecutable = $null
function Invoke-CapturedNativeProcess([string]$FilePath, [string[]]$Arguments, [int]$TimeoutMilliseconds, [hashtable]$ExtraEnvironment = @{}) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.WorkingDirectory = (Get-Location).Path
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { [void]$start.ArgumentList.Add($argument) }
    [void]$start.Environment.Remove('SYNVEIL_CLIENT_CONFIG')
    foreach ($entry in $ExtraEnvironment.GetEnumerator()) { $start.Environment[$entry.Key] = [string]$entry.Value }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        [void]$process.Start()
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutMilliseconds)) {
            try { $process.Kill($true) } catch { }
            throw 'P013_IPC_PROBE_TIMEOUT'
        }
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        if ($stdout.Length -gt 1048576 -or $stderr.Length -gt 1048576) { throw 'P013_IPC_PROBE_OUTPUT_LIMIT' }
        return [ordered]@{exit_code=$process.ExitCode;stdout=$stdout;stderr=$stderr}
    } finally { $process.Dispose() }
}
function Initialize-ControlIpcProbe([string]$RepositoryRoot, [string]$ExpectedHead) {
    if ($script:ipcProbeExecutable) { return }
    $git = Get-Command git -ErrorAction SilentlyContinue
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($null -eq $git -or $null -eq $cargo) { throw 'P013_IPC_PROBE_TOOLCHAIN_UNAVAILABLE' }
    $head = (& $git.Source -C $RepositoryRoot rev-parse HEAD 2>$null | Select-Object -First 1).Trim()
    if ($LASTEXITCODE -ne 0 -or $head -cne $ExpectedHead) { throw 'P013_IPC_PROBE_SOURCE_REVISION_MISMATCH' }
    $status = & $git.Source -C $RepositoryRoot status --porcelain --untracked-files=all 2>$null
    if ($LASTEXITCODE -ne 0 -or -not [string]::IsNullOrWhiteSpace(($status -join "`n"))) { throw 'P013_IPC_PROBE_SOURCE_TREE_DIRTY' }
    $script:ipcTempRoot = Join-Path $env:TEMP ('synveil-p013-ipc-' + [Guid]::NewGuid().ToString('N'))
    $cargoHome = Join-Path $script:ipcTempRoot 'cargo-home'
    $target = Join-Path $script:ipcTempRoot 'target'
    New-Item -ItemType Directory -Path $script:ipcTempRoot -Force | Out-Null
    $manifest = Join-Path $RepositoryRoot 'crates/client/Cargo.toml'
    $buildArgs = @('build','--quiet','--locked','--manifest-path',$manifest,'--example','p013_windows_control_probe')
    $buildEnv = @{CARGO_HOME=$cargoHome;CARGO_TARGET_DIR=$target}
    $build = Invoke-CapturedNativeProcess $cargo.Source $buildArgs 900000 $buildEnv
    if ($build.exit_code -ne 0) { throw 'P013_IPC_PROBE_BUILD_FAILED' }
    $script:ipcProbeExecutable = Join-Path (Join-Path (Join-Path $target 'debug') 'examples') 'p013_windows_control_probe.exe'
    if (-not (Test-Path -LiteralPath $script:ipcProbeExecutable -PathType Leaf)) { throw 'P013_IPC_PROBE_BINARY_MISSING' }
}
function Invoke-ControlIpcProbe([string]$RepositoryRoot, [string]$ExpectedHead, [string]$Mode, [string[]]$ModeArguments = @()) {
    Initialize-ControlIpcProbe $RepositoryRoot $ExpectedHead
    $extra = @{}
    if ($Mode -ceq 'shutdown') { $extra.P013_OPERATOR_APPROVAL = $approvalText }
    $capture = Invoke-CapturedNativeProcess $script:ipcProbeExecutable (@($Mode) + $ModeArguments) 20000 $extra
    $jsonLine = $capture.stdout -split "`r?`n" | Where-Object { $_ -match '^\s*\{' } | Select-Object -Last 1
    if ([string]::IsNullOrWhiteSpace($jsonLine)) { throw 'P013_IPC_PROBE_EVIDENCE_MISSING' }
    $probe = $jsonLine | ConvertFrom-Json
    if ($probe.schema_version -ne 1 -or $probe.mode -cne $Mode) { throw 'P013_IPC_PROBE_EVIDENCE_INCOMPLETE' }
    $probe | Add-Member -NotePropertyName harness_exit_code -NotePropertyValue ([int]$capture.exit_code)
    return $probe
}
function Assert-ControlProbe($Probe, [string]$Mode, [string[]]$RequiredChecks) {
    Assert-True ($Probe.mode -ceq $Mode -and $Probe.result -ceq 'PASS' -and $Probe.harness_exit_code -eq 0) 'P013_IPC_PROBE_EVIDENCE_INCOMPLETE'
    foreach ($required in $RequiredChecks) {
        Assert-True ($Probe.checks.$required -ceq 'PASS') 'P013_IPC_PROBE_REQUIRED_CHECK_FAILED'
    }
}
function Invoke-WrongUserControlProbe([string]$RepositoryRoot, [string]$ExpectedHead, [string]$OwnerSid, [string]$OwnerProfileId) {
    if ([string]::IsNullOrWhiteSpace($PeerUserName)) { throw 'P013_PEER_USER_REQUIRED' }
    $peerAccount = @(Get-CimInstance Win32_UserAccount -Filter "Name='$PeerUserName'" | Where-Object { $_.LocalAccount })
    if ($peerAccount.Count -ne 1 -or $peerAccount[0].Description -cne 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') {
        throw 'P013_PEER_USER_NOT_MARKED_DISPOSABLE'
    }
    $peerSid = [string]$peerAccount[0].SID
    Assert-True ($peerSid -cne $OwnerSid) 'P013_PEER_USER_MUST_DIFFER_FROM_OWNER'
    $adminGroup = Get-LocalGroup -SID 'S-1-5-32-544' -ErrorAction Stop
    $peerIsAdmin = @(Get-LocalGroupMember -Group $adminGroup | Where-Object { $_.SID.Value -ceq $peerSid }).Count -gt 0
    Assert-True (-not $peerIsAdmin) 'P013_PEER_USER_MUST_BE_STANDARD_USER'
    $peerIdentity = [Security.Principal.NTAccount]::new("$env:COMPUTERNAME\$PeerUserName")
    $credential = Get-Credential -UserName $peerIdentity.Value -Message 'Enter the disposable standard-user peer account password. It is used only for this bounded IPC denial check.'
    $probeRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonDocuments)) ('Synveil-P013-' + [Guid]::NewGuid().ToString('N'))
    $peerProcess = $null
    try {
        Initialize-ControlIpcProbe $RepositoryRoot $ExpectedHead
        $directoryAcl = [Security.AccessControl.DirectorySecurity]::new()
        $directoryAcl.SetAccessRuleProtection($true,$false)
        $directoryAcl.SetOwner([Security.Principal.SecurityIdentifier]::new($OwnerSid))
        $inherit = [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit
        foreach ($entry in @(@{sid=$OwnerSid;rights=[Security.AccessControl.FileSystemRights]::FullControl},
            @{sid='S-1-5-18';rights=[Security.AccessControl.FileSystemRights]::FullControl},
            @{sid='S-1-5-32-544';rights=[Security.AccessControl.FileSystemRights]::FullControl},
            @{sid=$peerSid;rights=[Security.AccessControl.FileSystemRights]::ReadAndExecute})) {
            $rule = [Security.AccessControl.FileSystemAccessRule]::new(
                [Security.Principal.SecurityIdentifier]::new($entry.sid),$entry.rights,$inherit,
                [Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow)
            $directoryAcl.AddAccessRule($rule)
        }
        New-Item -ItemType Directory -Path $probeRoot -Force | Out-Null
        Set-Acl -LiteralPath $probeRoot -AclObject $directoryAcl
        $peerExecutable = Join-Path $probeRoot 'p013_windows_control_probe.exe'
        Copy-Item -LiteralPath $script:ipcProbeExecutable -Destination $peerExecutable
        $ownerAcl = Get-Acl -LiteralPath $peerExecutable
        $ownerAcl.SetAccessRuleProtection($true,$false)
        foreach ($entry in @(@{sid=$OwnerSid;rights=[Security.AccessControl.FileSystemRights]::FullControl},
            @{sid='S-1-5-18';rights=[Security.AccessControl.FileSystemRights]::FullControl},
            @{sid='S-1-5-32-544';rights=[Security.AccessControl.FileSystemRights]::FullControl},
            @{sid=$peerSid;rights=[Security.AccessControl.FileSystemRights]::ReadAndExecute})) {
            $rule = [Security.AccessControl.FileSystemAccessRule]::new(
                [Security.Principal.SecurityIdentifier]::new($entry.sid),$entry.rights,
                [Security.AccessControl.AccessControlType]::Allow)
            $ownerAcl.AddAccessRule($rule)
        }
        Set-Acl -LiteralPath $peerExecutable -AclObject $ownerAcl
        $stdout = Join-Path $probeRoot 'peer-probe.stdout.json'
        $stderr = Join-Path $probeRoot 'peer-probe.stderr.log'
        $quoted = 'wrong-user "' + $OwnerProfileId + '"'
        $peerProcess = Start-Process -FilePath $peerExecutable -ArgumentList $quoted -WorkingDirectory $probeRoot `
            -Credential $credential -WindowStyle Hidden -RedirectStandardOutput $stdout `
            -RedirectStandardError $stderr -PassThru
        $peerWmi = Get-CimInstance Win32_Process -Filter "ProcessId=$($peerProcess.Id)" -ErrorAction Stop
        $peerOwner = Invoke-CimMethod -InputObject $peerWmi -MethodName GetOwnerSid -ErrorAction Stop
        Assert-True ($peerOwner.ReturnValue -eq 0 -and [string]$peerOwner.Sid -ceq $peerSid) 'P013_CROSS_USER_PROCESS_IDENTITY_MISMATCH'
        Assert-True (-not [SynveilP013GuiNative]::IsProcessElevated([uint32]$peerProcess.Id)) 'P013_CROSS_USER_PROCESS_ELEVATED'
        if (-not $peerProcess.WaitForExit(15000)) {
            try { $peerProcess.Kill($true) } catch { }
            throw 'P013_CROSS_USER_PROBE_TIMEOUT'
        }
        if ($peerProcess.ExitCode -ne 0) { throw 'P013_CROSS_USER_ACCESS_NOT_DENIED' }
        foreach ($log in @($stdout,$stderr)) {
            if ((Test-Path -LiteralPath $log) -and (Get-Item -LiteralPath $log).Length -gt 65536) {
                throw 'P013_CROSS_USER_LOG_LIMIT'
            }
        }
        $line = Get-Content -LiteralPath $stdout | Where-Object { $_ -match '^\s*\{' } | Select-Object -Last 1
        if ([string]::IsNullOrWhiteSpace($line)) { throw 'P013_CROSS_USER_EVIDENCE_MISSING' }
        $probe = $line | ConvertFrom-Json
        Assert-True ($probe.schema_version -eq 1 -and $probe.mode -ceq 'wrong-user' -and
            $probe.result -ceq 'PASS' -and $probe.checks.pipe_wrong_user -ceq 'PASS') 'P013_CROSS_USER_DENIAL_NOT_PROVEN'
        $probe | Add-Member -NotePropertyName peer_identity_sha256 -NotePropertyValue (
            [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($peerSid))).ToLowerInvariant())
        return $probe
    } finally {
        if ($null -ne $peerProcess -and -not $peerProcess.HasExited) { try { $peerProcess.Kill($true) } catch { } }
        if ($null -ne $credential) { try { $credential.Password.Dispose() } catch { } }
        $credential = $null
        if (Test-Path -LiteralPath $probeRoot) { Remove-Item -LiteralPath $probeRoot -Recurse -Force -ErrorAction Stop }
        if (Test-Path -LiteralPath $probeRoot) { throw 'P013_CROSS_USER_TEMP_CLEANUP_FAILURE' }
    }
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
$systemType = [string](Get-CimInstance Win32_ComputerSystem).SystemType
$script:observedArchitecture = if ($systemType -match '(?i)x64|64-bit') { 'x86_64' }
    elseif ($systemType -match '(?i)arm64') { 'arm64' } else { 'unknown' }
Assert-True ($os.Caption -match 'Windows 11' -and [int]$os.BuildNumber -ge 22000) 'P013_UNSUPPORTED_WINDOWS_BUILD'
Assert-True ($script:observedArchitecture -ceq 'x86_64') 'P013_UNSUPPORTED_ARCHITECTURE'
$interactiveEvidence = Get-CurrentInteractiveEvidence
if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true' -or -not $interactiveEvidence) {
    Write-BlockedGuiEvidence 'P013_BLOCKED_BY_ENVIRONMENT' $os $setupItem
    exit 2
}
Assert-True (-not (Test-Path Env:SYNVEIL_CLIENT_CONFIG)) 'P013_NONCANONICAL_PROFILE_OVERRIDE'
foreach ($name in @('QT_ROOT_DIR','QT_PLUGIN_PATH','QT_QPA_PLATFORM_PLUGIN_PATH','QT_QPA_PLATFORM','QT_QUICK_BACKEND','QML2_IMPORT_PATH','QML_IMPORT_PATH')) {
    Assert-True ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) 'P013_UNSANITIZED_GRAPHICS_ENVIRONMENT'
}
$testIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$testPrincipal = [Security.Principal.WindowsPrincipal]::new($testIdentity)
$elevated = [SynveilP013GuiNative]::IsElevated()
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$adminGroupPresent = @($testIdentity.Groups | Where-Object { $_.Value -eq $adminSid.Value }).Count -gt 0
Assert-True (-not $testPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -and -not $adminGroupPresent -and -not $elevated) 'P013_GUI_REQUIRES_STANDARD_USER'
$expectedSid = $testIdentity.User.Value
$localAccount = @(Get-CimInstance Win32_UserAccount -Filter "SID='$expectedSid'" | Where-Object { $_.LocalAccount })
Assert-True ($localAccount.Count -eq 1 -and $localAccount[0].Name -match '^synveil_p013_[0-9a-f]{8}$' -and
    $localAccount[0].Description -ceq 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') 'P013_DISPOSABLE_TEST_USER_REQUIRED'
$adminGroup = Get-LocalGroup -SID 'S-1-5-32-544' -ErrorAction Stop
$testUserIsAdmin = @(Get-LocalGroupMember -Group $adminGroup | Where-Object { $_.SID.Value -ceq $expectedSid }).Count -gt 0
Assert-True (-not $testUserIsAdmin) 'P013_GUI_REQUIRES_DISPOSABLE_STANDARD_USER'
$profile = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
$expectedRoot = Join-Path (Join-Path $profile 'Programs') 'Synveil'
$desktopPath = Join-Path $expectedRoot 'synveil-desktop.exe'
$roamingProductState = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) 'Synveil'
$uninstallKeyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
$uninstallSubkey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
Assert-True (-not (Test-Path -LiteralPath $expectedRoot) -and -not (Test-Path -LiteralPath $roamingProductState) -and
    -not (Test-Path -LiteralPath $uninstallKeyPath) -and -not (Test-Path -LiteralPath (Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::StartMenu)) 'Programs\Synveil.lnk')) -and
    (Get-HklmRegistrationCount $uninstallSubkey) -eq 0 -and
    @((Get-Service -Name '*Synveil*' -ErrorAction SilentlyContinue)).Count -eq 0 -and
    @((Get-ScheduledTask -TaskPath '\Synveil\BackgroundClient\' -ErrorAction SilentlyContinue)).Count -eq 0) `
    'P013_NONDISPOSABLE_PRODUCT_STATE_REJECTED'
$deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
$states = [ordered]@{
    candidate_identity = 'PASS'; setup_as_standard_user = 'NOT_RUN'; welcome = 'NOT_RUN'
    user_options = 'NOT_RUN'; install_location = 'NOT_RUN'; install_confirmation = 'NOT_RUN'
    install_progress = 'NOT_RUN'; completion = 'NOT_RUN'; installed_runtime = 'NOT_RUN'
    installed_launch = 'NOT_RUN'; first_run = 'NOT_RUN'; unexpected_uac = 'NOT_RUN'
    disposable_test_user = 'PASS'; installed_client_process = 'NOT_RUN'; pipe_protocol = 'NOT_RUN'
    installed_desktop_ipc_ping = 'NOT_RUN'
    pipe_wrong_user = 'NOT_RUN'; pipe_unauthorized_operation = 'NOT_RUN'; pipe_stale_profile = 'NOT_RUN'
    pipe_server_termination = 'NOT_RUN'; pipe_expired_endpoint = 'NOT_RUN'; pipe_restart = 'NOT_RUN'
    no_tcp_fallback = 'NOT_RUN'; shutdown = 'NOT_RUN'; ipc_harness_cleanup = 'NOT_RUN'
    setup_non_elevated = 'NOT_RUN'; installed_desktop_non_elevated = 'NOT_RUN'
    installed_client_non_elevated = 'NOT_RUN'; uninstall_preservation = 'NOT_RUN'
}
$setupProcess = $null
$desktopProcess = $null
$restartedClient = $null
$errorCode = $null
$observedChoices = $null
$controlIpcEvidence = $null
try {
    $setupProcess = Start-Process -FilePath $setupPath -PassThru
    $setupProcess.Refresh()
    $setupOwnerMatches = (Get-UserSidForProcess $setupProcess) -ceq $expectedSid
    $setupElevated = [SynveilP013GuiNative]::IsProcessElevated([uint32]$setupProcess.Id)
    $states.setup_non_elevated = if (-not $setupElevated) { 'PASS' } else { 'FAIL' }
    $states.setup_as_standard_user = if ($setupOwnerMatches -and -not $setupElevated) { 'PASS' } else { 'FAIL' }
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
    $null = Wait-ForUiStage $desktopProcess 'Installed background IPC Ping' '^Ready$' ([DateTimeOffset]::UtcNow.AddSeconds(45))
    $states.installed_desktop_ipc_ping = 'PASS'
    Assert-True ((Get-UserSidForProcess $desktopProcess) -ceq $expectedSid) 'P013_INSTALLED_PROCESS_USER_MISMATCH'
    $desktopElevated = [SynveilP013GuiNative]::IsProcessElevated([uint32]$desktopProcess.Id)
    $states.installed_desktop_non_elevated = if (-not $desktopElevated) { 'PASS' } else { 'FAIL' }
    Assert-True (-not $desktopElevated) 'P013_INSTALLED_DESKTOP_ELEVATED'
    Assert-True ((Resolve-Path -LiteralPath $desktopPath).Path -ceq [IO.Path]::GetFullPath($desktopPath)) 'P013_INSTALLED_RUNTIME_PATH_MISMATCH'
    $duplicateApps = @(Get-Process -Name 'synveil-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $desktopProcess.Id })
    Assert-True ($duplicateApps.Count -eq 0) 'P013_UNEXPECTED_DUPLICATE_DESKTOP_PROCESS'
    $states.installed_launch = 'PASS'; $states.first_run = 'PASS'; $states.unexpected_uac = 'PASS'

    $clientPath = Join-Path $expectedRoot 'synveil-client.exe'
    $clientDeadline = [DateTimeOffset]::UtcNow.AddSeconds(30)
    $clientProcesses = @()
    while ([DateTimeOffset]::UtcNow -lt $clientDeadline) {
        $clientProcesses = @(Get-Process -Name 'synveil-client' -ErrorAction SilentlyContinue | Where-Object {
            try { $_.Path -ceq $clientPath } catch { $false }
        })
        if ($clientProcesses.Count -eq 1) { break }
        Start-Sleep -Milliseconds 200
    }
    Assert-True ($clientProcesses.Count -eq 1) 'P013_INSTALLED_CLIENT_PROCESS_MISSING_OR_DUPLICATED'
    Assert-True ((Get-UserSidForProcess $clientProcesses[0]) -ceq $expectedSid) 'P013_INSTALLED_CLIENT_USER_MISMATCH'
    $clientElevated = [SynveilP013GuiNative]::IsProcessElevated([uint32]$clientProcesses[0].Id)
    $states.installed_client_non_elevated = if (-not $clientElevated) { 'PASS' } else { 'FAIL' }
    Assert-True (-not $clientElevated) 'P013_INSTALLED_CLIENT_ELEVATED'
    $states.installed_client_process = 'PASS'
    $repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
    $ownerProfileId = Get-CurrentProfileId $expectedSid
    $ownerProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'owner'
    Assert-ControlProbe $ownerProbe 'owner' @('pipe_endpoint_discovery','pipe_protocol_v1','pipe_ping',
        'pipe_unsupported_version','pipe_malformed_message','pipe_oversized_payload','pipe_duplicate_connection',
        'pipe_server_handshake_timeout','pipe_timeout_cancellation')
    $staleProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'stale'
    Assert-ControlProbe $staleProbe 'stale' @('pipe_stale_profile')
    $controlIpcEvidence = [ordered]@{owner=$ownerProbe;stale_profile=$staleProbe}
    $states.pipe_protocol = 'PASS'; $states.pipe_stale_profile = 'PASS'
    if ([string]::IsNullOrWhiteSpace($PeerUserName)) {
        $states.pipe_wrong_user = 'BLOCKED_BY_ENVIRONMENT'
        $states.pipe_unauthorized_operation = 'BLOCKED_BY_ENVIRONMENT'
        throw 'P013_BLOCKED_BY_ENVIRONMENT: disposable standard-user peer account was not supplied'
    }
    $wrongUserProbe = Invoke-WrongUserControlProbe $repoRoot $ExpectedHarnessCommit $expectedSid $ownerProfileId
    $controlIpcEvidence.cross_user = $wrongUserProbe
    $states.pipe_wrong_user = 'PASS'; $states.pipe_unauthorized_operation = 'PASS'
    $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object {
        $_.OwningProcess -in @($clientProcesses[0].Id,$desktopProcess.Id)
    })
    Assert-True ($listeners.Count -eq 0) 'P013_UNEXPECTED_TCP_FALLBACK'
    $states.no_tcp_fallback = 'PASS'

    $window = Get-ProcessWindow $desktopProcess
    if ($null -ne $window) {
        $closePattern = $null
        if ($window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$closePattern)) {
            ([System.Windows.Automation.WindowPattern]$closePattern).Close()
        } else { $desktopProcess.CloseMainWindow() | Out-Null }
    } else { $desktopProcess.CloseMainWindow() | Out-Null }
    if (-not $desktopProcess.WaitForExit(15000)) { $desktopProcess.Kill(); throw 'P013_DESKTOP_SHUTDOWN_TIMEOUT' }
    $states.shutdown = 'PASS'

    $shutdownProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'shutdown'
    Assert-ControlProbe $shutdownProbe 'shutdown' @('pipe_server_termination')
    $controlIpcEvidence.shutdown = $shutdownProbe
    if (-not $clientProcesses[0].WaitForExit(15000)) { throw 'P013_PIPE_SERVER_SHUTDOWN_TIMEOUT' }
    $states.pipe_server_termination = 'PASS'
    $absentProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'absent'
    Assert-ControlProbe $absentProbe 'absent' @('pipe_expired_endpoint')
    $controlIpcEvidence.endpoint_absent = $absentProbe
    $states.pipe_expired_endpoint = 'PASS'

    $restartedClient = Start-Process -FilePath $clientPath -WorkingDirectory $expectedRoot -PassThru
    Assert-True ((Get-UserSidForProcess $restartedClient) -ceq $expectedSid) 'P013_RESTARTED_CLIENT_USER_MISMATCH'
    $restartDeadline = [DateTimeOffset]::UtcNow.AddSeconds(30)
    $restartProbe = $null
    while ([DateTimeOffset]::UtcNow -lt $restartDeadline) {
        $restartedClient.Refresh()
        if ($restartedClient.HasExited) { throw 'P013_RESTARTED_CLIENT_EXITED' }
        $runtimeClients = @(Get-Process -Name 'synveil-client' -ErrorAction SilentlyContinue | Where-Object {
            try { $_.Path -ceq $clientPath } catch { $false }
        })
        if ($runtimeClients.Count -ne 1) { Start-Sleep -Milliseconds 200; continue }
        $candidateProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'ping'
        if ($candidateProbe.result -ceq 'PASS' -and $candidateProbe.checks.pipe_ping -ceq 'PASS') {
            $restartProbe = $candidateProbe
            break
        }
        Start-Sleep -Milliseconds 500
    }
    Assert-True ($null -ne $restartProbe) 'P013_PIPE_SERVER_RESTART_FAILED'
    Assert-ControlProbe $restartProbe 'ping' @('pipe_endpoint_discovery','pipe_protocol_v1','pipe_ping')
    $controlIpcEvidence.restart_ping = $restartProbe
    $states.pipe_restart = 'PASS'
    $listenersAfterRestart = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object {
        $_.OwningProcess -in @($restartedClient.Id)
    })
    Assert-True ($listenersAfterRestart.Count -eq 0) 'P013_UNEXPECTED_TCP_FALLBACK_AFTER_RESTART'
    $states.no_tcp_fallback = 'PASS'

    $restartShutdownProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'shutdown'
    Assert-ControlProbe $restartShutdownProbe 'shutdown' @('pipe_server_termination')
    $controlIpcEvidence.restart_shutdown = $restartShutdownProbe
    if (-not $restartedClient.WaitForExit(15000)) { throw 'P013_RESTARTED_CLIENT_SHUTDOWN_TIMEOUT' }
    $restartAbsentProbe = Invoke-ControlIpcProbe $repoRoot $ExpectedHarnessCommit 'absent'
    Assert-ControlProbe $restartAbsentProbe 'absent' @('pipe_expired_endpoint')
    $controlIpcEvidence.restart_endpoint_absent = $restartAbsentProbe

    $appDataRoot = [Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)
    $stateRoot = Join-Path $appDataRoot 'Synveil'
    $stateSentinel = Join-Path $stateRoot 'p013-uninstall-preservation.txt'
    $sentinelBytes = [Text.Encoding]::UTF8.GetBytes('SYNVEIL P013 non-secret uninstall preservation sentinel')
    [IO.File]::WriteAllBytes($stateSentinel, $sentinelBytes)
    $sentinelHash = (Get-FileHash -LiteralPath $stateSentinel -Algorithm SHA256).Hash.ToLowerInvariant()
    . (Join-Path $PSScriptRoot 'windows-security.ps1')
    $registered = Get-ItemProperty -LiteralPath $uninstallKeyPath -ErrorAction Stop
    $uninstaller = Get-OwnedRegisteredUninstaller ([string]$registered.UninstallString) $expectedRoot
    $uninstallerProcess = Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART') -PassThru
    Assert-True (-not [SynveilP013GuiNative]::IsProcessElevated([uint32]$uninstallerProcess.Id)) 'P013_UNINSTALLER_ELEVATED'
    if (-not $uninstallerProcess.WaitForExit(60000)) {
        try { $uninstallerProcess.Kill() } catch { }
        throw 'P013_UNINSTALL_TIMEOUT'
    }
    Assert-True ($uninstallerProcess.ExitCode -eq 0) 'P013_UNINSTALL_FAILURE'
    $removeDeadline = [DateTimeOffset]::UtcNow.AddSeconds(15)
    while ((Test-Path -LiteralPath $expectedRoot) -and [DateTimeOffset]::UtcNow -lt $removeDeadline) { Start-Sleep -Milliseconds 100 }
    Assert-True (-not (Test-Path -LiteralPath $expectedRoot) -and -not (Test-Path -LiteralPath $uninstallKeyPath) -and
        (Get-HklmRegistrationCount $uninstallSubkey) -eq 0 -and
        @((Get-Service -Name '*Synveil*' -ErrorAction SilentlyContinue)).Count -eq 0 -and
        @((Get-ScheduledTask -TaskPath '\Synveil\BackgroundClient\' -ErrorAction SilentlyContinue)).Count -eq 0) `
        'P013_UNINSTALL_PACKAGE_REMAINS'
    Assert-True ((Test-Path -LiteralPath $stateSentinel -PathType Leaf) -and
        (Get-FileHash -LiteralPath $stateSentinel -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $sentinelHash) 'P013_UNINSTALL_STATE_NOT_PRESERVED'
    $states.uninstall_preservation = 'PASS'
} catch {
    $errorCode = if ($_.Exception.Message -match '^P013_[A-Z0-9_]+') { $Matches[0] } else { 'P013_GUI_ACCEPTANCE_FAILURE' }
    if ($desktopProcess -and -not $desktopProcess.HasExited) {
        try { $desktopProcess.Kill(); $states.shutdown = 'FAIL' } catch { $states.shutdown = 'FAIL' }
    }
    if ($setupProcess -and -not $setupProcess.HasExited) {
        try { $setupProcess.Kill(); $states.completion = 'FAIL' } catch { $states.completion = 'FAIL' }
    }
}

if ($script:ipcTempRoot) {
    try {
        Remove-Item -LiteralPath $script:ipcTempRoot -Recurse -Force -ErrorAction Stop
        if (Test-Path -LiteralPath $script:ipcTempRoot) { throw 'P013_IPC_HARNESS_TEMP_CLEANUP_FAILURE' }
        $states.ipc_harness_cleanup = 'PASS'
    } catch { $states.ipc_harness_cleanup = 'FAIL'; if (-not $errorCode) { $errorCode = 'P013_IPC_HARNESS_TEMP_CLEANUP_FAILURE' } }
}
$result = if ($errorCode -eq 'P013_BLOCKED_BY_ENVIRONMENT') { 'BLOCKED_BY_ENVIRONMENT' }
    elseif ($errorCode) { 'FAIL' }
    elseif (@($states.Values | Where-Object { $_ -ne 'PASS' }).Count -eq 0) { 'PASS' }
    else { 'INCOMPLETE' }
$repoRootForEvidence = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$harnessFiles = @(
    @{name='scripts/test-windows-gui-journey.ps1';path=$PSCommandPath},
    @{name='crates/client/examples/p013_windows_control_probe.rs';path=(Join-Path $repoRootForEvidence 'crates/client/examples/p013_windows_control_probe.rs')}
)
$harnessBinding = ($harnessFiles | Sort-Object name | ForEach-Object {
    $_.name + ':' + (Get-FileHash -LiteralPath $_.path -Algorithm SHA256).Hash.ToLowerInvariant()
}) -join "`n"
$harnessHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($harnessBinding))).ToLowerInvariant()
$uiAssertionNames = @('welcome','user_options','install_location','install_confirmation','install_progress','completion','installed_launch','first_run','installed_desktop_ipc_ping')
$uiAssertionsPassed = @($uiAssertionNames | Where-Object { $states[$_] -ne 'PASS' }).Count -eq 0
$evidence = [ordered]@{
    schema_version=1; result=$result; execution_id='p013-ui-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    source_commit=$ExpectedSourceCommit;harness_source_commit=$ExpectedHarnessCommit
    candidate=[ordered]@{sha256=$setupHash;size_bytes=$setupItem.Length;release_manifest_sha256=$manifestHash}
    harness_revision_sha256=$harnessHash;windows_edition=[string]$os.Caption
    windows_build=[int]$os.BuildNumber;architecture=$script:observedArchitecture;privilege='standard_user_non_elevated';token_elevated=[bool]$elevated
    process_session_id=[uint32]$script:observedSessionId
    product_state=$states;ui_evidence=[ordered]@{mode='native_ui_automation';state_assertions=$uiAssertionsPassed;screenshots_status='NOT_CAPTURED'}
    user_options=$observedChoices;control_ipc=$controlIpcEvidence;cleanup_status='NOT_RUN';redacted=$true;error_code=$errorCode
    user_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($expectedSid))).ToLowerInvariant()
}
$evidencePath = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-gui-journey-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $evidencePath) -Force | Out-Null
$evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $evidencePath -Encoding utf8
Write-Host "P013 GUI journey: $result"
Write-Host "Evidence: $evidencePath"
if ($result -ne 'PASS') { exit 1 }
