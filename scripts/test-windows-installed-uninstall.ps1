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
    [Parameter(Mandatory=$true)][string]$OperatorApproval
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
$checks = [ordered]@{
    candidate_identity='NOT_RUN';harness_identity='NOT_RUN';windows_build_policy='NOT_RUN'
    interactive_standard_user='NOT_RUN';installed_runtime_identity='NOT_RUN'
    profile_owner='NOT_RUN';startup_task_identity='NOT_RUN';installed_client_identity='NOT_RUN'
    client_protocol_shutdown='NOT_RUN';pipe_expired_after_shutdown='NOT_RUN'
    uninstaller_identity='NOT_RUN';uninstaller_non_elevated='NOT_RUN'
    package_removed='NOT_RUN';uninstall_preservation='NOT_RUN';process_absent='NOT_RUN'
    probe_temp_cleanup='NOT_RUN'
}
$result = 'INCOMPLETE'
$errorCode = $null
$setupHash = $null
$manifestHash = $null
$setupSize = $null
$os = $null
$identity = $null
$elevated = $null
$sidHash = $null
$sessionId = 0
$probeRoot = $null
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path

$nativeType = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class SynveilP013UninstallNative {
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, out int value, int length, out int needed);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool ProcessIdToSessionId(uint pid, out uint session);
  [DllImport("user32.dll", SetLastError=true)] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll", SetLastError=true)] static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool GetUserObjectInformation(IntPtr desktop, int index, StringBuilder value, int length, out int needed);
  [DllImport("wtsapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WTSQuerySessionInformation(IntPtr server, uint session, int infoClass, out IntPtr buffer, out uint bytes);
  [DllImport("wtsapi32.dll")] static extern void WTSFreeMemory(IntPtr buffer);
  static bool Elevated(IntPtr process) {
    IntPtr token; int value, needed;
    if (!OpenProcessToken(process, 0x0008, out token)) throw new Win32Exception();
    try { if (!GetTokenInformation(token, 20, out value, sizeof(int), out needed)) throw new Win32Exception(); return value != 0; }
    finally { CloseHandle(token); }
  }
  public static bool CurrentIsElevated() { return Elevated(GetCurrentProcess()); }
  public static bool ProcessIsElevated(uint pid) {
    IntPtr process = OpenProcess(0x1000, false, pid);
    if (process == IntPtr.Zero) throw new Win32Exception();
    try { return Elevated(process); } finally { CloseHandle(process); }
  }
  public static bool GetSession(uint pid, out uint session) { return ProcessIdToSessionId(pid, out session); }
  public static bool DefaultInputDesktop() {
    IntPtr desktop = OpenInputDesktop(0, false, 1); if (desktop == IntPtr.Zero) return false;
    try { StringBuilder name = new StringBuilder(128); int needed; return GetUserObjectInformation(desktop, 2, name, name.Capacity, out needed) && name.ToString() == "Default"; }
    finally { CloseDesktop(desktop); }
  }
  public static bool ActiveSession(uint session) {
    IntPtr buffer; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 8, out buffer, out bytes)) return false;
    try { return Marshal.ReadInt32(buffer) == 0; } finally { WTSFreeMemory(buffer); }
  }
  public static string WtsText(uint session, int infoClass) {
    IntPtr buffer; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, infoClass, out buffer, out bytes)) return null;
    try { return Marshal.PtrToStringUni(buffer); } finally { WTSFreeMemory(buffer); }
  }
}
'@
if (-not ('SynveilP013UninstallNative' -as [type])) { Add-Type -TypeDefinition $nativeType }

function Assert-P013([bool]$Condition, [string]$Code) {
    if (-not $Condition) { throw $Code }
}
function Hash-P013([string]$Value) {
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value))).ToLowerInvariant()
}
function Resolve-P013Sid([string]$Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) { return $null }
    if ($Value -match '^S-1-') { return $Value }
    try { return ([Security.Principal.NTAccount]::new($Value)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { return $null }
}
function Get-ProcessSid([Diagnostics.Process]$Process) {
    $item = Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)" -ErrorAction Stop
    $owner = Invoke-CimMethod -InputObject $item -MethodName GetOwnerSid -ErrorAction Stop
    if ($owner.ReturnValue -ne 0) { throw 'P013_UNINSTALL_PROCESS_IDENTITY_UNAVAILABLE' }
    return [string]$owner.Sid
}
function Get-OwnedClientProcesses([string]$ClientPath, [string]$ExpectedSid) {
    $found = [Collections.Generic.List[object]]::new()
    foreach ($item in @(Get-CimInstance Win32_Process -Filter "Name='synveil-client.exe'")) {
        if ([string]$item.ExecutablePath -ine $ClientPath) { continue }
        $owner = Invoke-CimMethod -InputObject $item -MethodName GetOwnerSid -ErrorAction Stop
        if ($owner.ReturnValue -ne 0) { throw 'P013_UNINSTALL_PROCESS_IDENTITY_UNAVAILABLE' }
        if ([string]$owner.Sid -ceq $ExpectedSid) { $found.Add($item) }
    }
    return @($found)
}
function Invoke-BoundedNative([string]$FilePath, [string[]]$Arguments, [int]$TimeoutMilliseconds, [hashtable]$ExtraEnvironment = @{}) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.WorkingDirectory = $repoRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { [void]$start.ArgumentList.Add($argument) }
    foreach ($entry in $ExtraEnvironment.GetEnumerator()) { $start.Environment[$entry.Key] = [string]$entry.Value }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        [void]$process.Start()
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutMilliseconds)) {
            try { $process.Kill($true) } catch { }
            throw 'P013_UNINSTALL_HARNESS_TIMEOUT'
        }
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        if ($stdout.Length -gt 65536 -or $stderr.Length -gt 65536) { throw 'P013_UNINSTALL_HARNESS_OUTPUT_LIMIT' }
        return [ordered]@{exit_code=$process.ExitCode;stdout=$stdout;stderr=$stderr}
    } finally { $process.Dispose() }
}
function Invoke-PipeProbe([string]$Mode, [string]$ProbePath) {
    $extra = @{}
    if ($Mode -ceq 'shutdown') { $extra.P013_OPERATOR_APPROVAL = $approvalText }
    $capture = Invoke-BoundedNative $ProbePath @($Mode) 20000 $extra
    $line = $capture.stdout -split "`r?`n" | Where-Object { $_ -match '^\s*\{' } | Select-Object -Last 1
    if ([string]::IsNullOrWhiteSpace($line)) { throw 'P013_UNINSTALL_PROBE_EVIDENCE_MISSING' }
    $record = $line | ConvertFrom-Json
    if ($record.schema_version -ne 1 -or $record.mode -cne $Mode -or $record.result -cne 'PASS' -or $capture.exit_code -ne 0) {
        throw 'P013_UNINSTALL_PROBE_FAILED'
    }
    return $record
}
function Get-HklmRegistrationCount([string]$Subkey) {
    $count = 0
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,$view)
        try { $key = $base.OpenSubKey($Subkey); if ($null -ne $key) { $count++; $key.Dispose() } }
        finally { $base.Dispose() }
    }
    return $count
}
function Assert-ProfileTask([string]$TaskName, [string]$Sid, [string]$ClientPath, [string]$RuntimeRoot) {
    $task = Get-ScheduledTask -TaskPath '\Synveil\BackgroundClient\' -TaskName $TaskName -ErrorAction Stop
    $xml = [xml](Export-ScheduledTask -TaskPath $task.TaskPath -TaskName $task.TaskName -ErrorAction Stop)
    $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
    $ns.AddNamespace('t','http://schemas.microsoft.com/windows/2004/02/mit/task')
    $trigger = @($xml.SelectNodes('/t:Task/t:Triggers/t:LogonTrigger',$ns))
    $principal = Resolve-P013Sid ([string]$xml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:UserId',$ns).InnerText)
    $triggerSid = if ($trigger.Count -eq 1) { Resolve-P013Sid ([string]$trigger[0].UserId) } else { $null }
    $actions = @($task.Actions)
    Assert-P013 ($trigger.Count -eq 1 -and $trigger[0].Enabled -ceq 'true' -and $principal -ceq $Sid -and $triggerSid -ceq $Sid -and
        [string]$xml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:LogonType',$ns).InnerText -ceq 'InteractiveToken' -and
        [string]$xml.SelectSingleNode('/t:Task/t:Principals/t:Principal/t:RunLevel',$ns).InnerText -ceq 'LeastPrivilege' -and
        $actions.Count -eq 1 -and [IO.Path]::GetFullPath([string]$actions[0].Execute) -ieq [IO.Path]::GetFullPath($ClientPath) -and
        [string]$actions[0].Arguments -eq '' -and [IO.Path]::GetFullPath([string]$actions[0].WorkingDirectory) -ieq [IO.Path]::GetFullPath($RuntimeRoot) -and
        $xml.SelectNodes('//t:Password',$ns).Count -eq 0) 'P013_UNINSTALL_TASK_IDENTITY_MISMATCH'
}

try {
    Assert-P013 ($OperatorApproval -ceq $approvalText) 'P013_OPERATOR_APPROVAL_REQUIRED'
    Assert-P013 ($PSVersionTable.PSVersion.Major -ge 7) 'P013_REQUIRES_POWERSHELL_7'
    Assert-P013 ($env:GITHUB_ACTIONS -ne 'true' -and -not $env:RUNNER_NAME -and $env:CI -ne 'true') 'P013_BLOCKED_BY_ENVIRONMENT: hosted session rejected'
    Assert-P013 (-not (Test-Path Env:SYNVEIL_CLIENT_CONFIG)) 'P013_NONCANONICAL_PROFILE_OVERRIDE'

    $setupPath = (Resolve-Path -LiteralPath $Setup).Path
    $manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest).Path
    $setupHash = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $setupSize = (Get-Item -LiteralPath $setupPath).Length
    $release = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $artifact = @($release.artifacts | Where-Object { $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe' })
    Assert-P013 ($setupHash -ceq $ExpectedSetupSha256 -and $setupSize -eq $ExpectedSetupSizeBytes -and
        $manifestHash -ceq $ExpectedManifestSha256 -and $release.schema_version -eq 1 -and
        $release.product -ceq 'Synveil' -and $release.source_commit -ceq $ExpectedSourceCommit -and
        $artifact.Count -eq 1 -and $artifact[0].sha256 -ceq $setupHash -and [long]$artifact[0].size_bytes -eq $setupSize -and
        (Get-AuthenticodeSignature -FilePath $setupPath).Status -eq 'NotSigned') 'P013_CANDIDATE_IDENTITY_MISMATCH'
    $checks.candidate_identity = 'PASS'

    $git = Get-Command git -ErrorAction SilentlyContinue
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    Assert-P013 ($null -ne $git -and $null -ne $cargo) 'P013_UNINSTALL_TOOLCHAIN_UNAVAILABLE'
    $head = (& $git.Source -C $repoRoot rev-parse HEAD 2>$null | Select-Object -First 1).Trim()
    $gitStatus = & $git.Source -C $repoRoot status --porcelain --untracked-files=all 2>$null
    Assert-P013 ($LASTEXITCODE -eq 0 -and $head -ceq $ExpectedHarnessCommit -and [string]::IsNullOrWhiteSpace(($gitStatus -join "`n"))) 'P013_UNINSTALL_HARNESS_REVISION_MISMATCH'
    $checks.harness_identity = 'PASS'

    $os = Get-CimInstance Win32_OperatingSystem
    $systemType = [string](Get-CimInstance Win32_ComputerSystem).SystemType
    $isX64 = $systemType -match '(?i)x64|64-bit'
    Assert-P013 ($os.Caption -match 'Windows 11' -and [int]$os.BuildNumber -ge 22000 -and $isX64) 'P013_UNINSTALL_UNSUPPORTED_ENVIRONMENT'
    $checks.windows_build_policy = 'PASS'
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $sid = $identity.User.Value
    $sidHash = Hash-P013 $sid
    $adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    $elevated = [SynveilP013UninstallNative]::CurrentIsElevated()
    Assert-P013 (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -and
        @($identity.Groups | Where-Object { $_.Value -eq $adminSid.Value }).Count -eq 0 -and
        -not $elevated) 'P013_UNINSTALL_REQUIRES_STANDARD_USER'
    $session = [uint32]0
    Assert-P013 ([SynveilP013UninstallNative]::GetSession([uint32]$PID,[ref]$session) -and $session -ne 0 -and
        [SynveilP013UninstallNative]::DefaultInputDesktop() -and [SynveilP013UninstallNative]::ActiveSession($session)) 'P013_BLOCKED_BY_ENVIRONMENT: interactive session unavailable'
    $sessionId = [int]$session
    $wtsUser = [SynveilP013UninstallNative]::WtsText($session,5)
    $wtsDomain = [SynveilP013UninstallNative]::WtsText($session,7)
    $wtsAccount = if ([string]::IsNullOrWhiteSpace($wtsDomain)) { $wtsUser } else { "$wtsDomain\$wtsUser" }
    Assert-P013 ((Resolve-P013Sid $wtsAccount) -ceq $sid) 'P013_BLOCKED_BY_ENVIRONMENT: session identity mismatch'
    $account = @(Get-CimInstance Win32_UserAccount -Filter "SID='$sid'" | Where-Object { $_.LocalAccount })
    Assert-P013 ($account.Count -eq 1 -and $account[0].Name -match '^synveil_p013_[0-9a-f]{8}$' -and
        $account[0].Description -ceq 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') 'P013_UNINSTALL_DISPOSABLE_USER_REQUIRED'
    $checks.interactive_standard_user = 'PASS'

    $profileRoot = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    $runtimeRoot = Join-Path (Join-Path $profileRoot 'Programs') 'Synveil'
    $clientPath = Join-Path $runtimeRoot 'synveil-client.exe'
    $desktopPath = Join-Path $runtimeRoot 'synveil-desktop.exe'
    $roamingRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) 'Synveil'
    $configPath = Join-Path $roamingRoot 'client.conf'
    Assert-P013 ((Test-Path -LiteralPath $clientPath -PathType Leaf) -and (Test-Path -LiteralPath $desktopPath -PathType Leaf) -and
        (Test-Path -LiteralPath $configPath -PathType Leaf)) 'P013_UNINSTALL_INSTALLED_PRODUCT_MISSING'
    . (Join-Path $PSScriptRoot 'windows-security.ps1')
    Assert-NoReparseAncestry $runtimeRoot
    Assert-NoReparseAncestry $configPath $true
    $configOwner = ([Security.Principal.NTAccount]::new((Get-Acl -LiteralPath $configPath).Owner)).Translate([Security.Principal.SecurityIdentifier]).Value
    Assert-P013 ($configOwner -ceq $sid) 'P013_UNINSTALL_PROFILE_OWNER_MISMATCH'
    $profileLines = @([IO.File]::ReadAllLines($configPath) | Where-Object { $_ -match '^\s*profile_id\s*=' })
    Assert-P013 ($profileLines.Count -eq 1 -and $profileLines[0] -match '^\s*profile_id\s*=\s*([0-9a-f-]{36})\s*$') 'P013_UNINSTALL_PROFILE_ID_UNAVAILABLE'
    $profileId = $Matches[1].ToLowerInvariant()
    Assert-P013 ($profileId -match '^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$') 'P013_UNINSTALL_PROFILE_ID_UNAVAILABLE'
    $checks.profile_owner = 'PASS'
    & (Join-Path $PSScriptRoot 'test-windows-installed-runtime.ps1') -RuntimeRoot $runtimeRoot -Manifest (Join-Path $runtimeRoot 'SYNVEIL-MANIFEST.txt')
    if ($LASTEXITCODE -ne 0) { throw 'P013_UNINSTALL_RUNTIME_IDENTITY_FAILURE' }
    $checks.installed_runtime_identity = 'PASS'

    $taskPath = '\Synveil\BackgroundClient\'
    $taskName = 'profile-' + $profileId
    Assert-ProfileTask $taskName $sid $clientPath $runtimeRoot
    $checks.startup_task_identity = 'PASS'
    $desktopProcesses = @(Get-CimInstance Win32_Process -Filter "Name='synveil-desktop.exe'" | Where-Object { $_.ExecutablePath -ieq $desktopPath })
    Assert-P013 ($desktopProcesses.Count -eq 0) 'P013_UNINSTALL_DESKTOP_STILL_RUNNING'
    $clientProcesses = @(Get-OwnedClientProcesses $clientPath $sid)
    Assert-P013 ($clientProcesses.Count -eq 1) 'P013_UNINSTALL_CLIENT_MISSING_OR_DUPLICATED'
    Assert-P013 (-not [SynveilP013UninstallNative]::ProcessIsElevated([uint32]$clientProcesses[0].ProcessId)) 'P013_UNINSTALL_CLIENT_ELEVATED'
    $checks.installed_client_identity = 'PASS'

    $probeRoot = Join-Path $env:TEMP ('synveil-p013-uninstall-' + [Guid]::NewGuid().ToString('N'))
    $cargoHome = Join-Path $probeRoot 'cargo-home'
    $target = Join-Path $probeRoot 'target'
    New-Item -ItemType Directory -Path $probeRoot -Force | Out-Null
    $build = Invoke-BoundedNative $cargo.Source @('build','--quiet','--locked','--manifest-path',(Join-Path $repoRoot 'crates/client/Cargo.toml'),'--example','p013_windows_control_probe') 900000 @{CARGO_HOME=$cargoHome;CARGO_TARGET_DIR=$target}
    Assert-P013 ($build.exit_code -eq 0) 'P013_UNINSTALL_PROBE_BUILD_FAILED'
    $probePath = Join-Path (Join-Path (Join-Path $target 'debug') 'examples') 'p013_windows_control_probe.exe'
    Assert-P013 (Test-Path -LiteralPath $probePath -PathType Leaf) 'P013_UNINSTALL_PROBE_BINARY_MISSING'
    $shutdown = Invoke-PipeProbe 'shutdown' $probePath
    Assert-P013 ($shutdown.checks.pipe_server_termination -ceq 'PASS') 'P013_UNINSTALL_CLIENT_SHUTDOWN_NOT_ACKNOWLEDGED'
    $checks.client_protocol_shutdown = 'PASS'
    $exitDeadline = [DateTimeOffset]::UtcNow.AddSeconds(15)
    do {
        $clientProcesses = @(Get-OwnedClientProcesses $clientPath $sid)
        if ($clientProcesses.Count -eq 0) { break }
        Start-Sleep -Milliseconds 150
    } while ([DateTimeOffset]::UtcNow -lt $exitDeadline)
    Assert-P013 ($clientProcesses.Count -eq 0) 'P013_UNINSTALL_CLIENT_SHUTDOWN_TIMEOUT'
    $absent = Invoke-PipeProbe 'absent' $probePath
    Assert-P013 ($absent.checks.pipe_expired_endpoint -ceq 'PASS') 'P013_UNINSTALL_ENDPOINT_REMAINS'
    $checks.pipe_expired_after_shutdown = 'PASS'
    $uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
    $uninstallSubkey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
    $registered = Get-ItemProperty -LiteralPath $uninstallKey -ErrorAction Stop
    $uninstaller = Get-OwnedRegisteredUninstaller ([string]$registered.UninstallString) $runtimeRoot
    $checks.uninstaller_identity = 'PASS'
    Assert-P013 ((Get-HklmRegistrationCount $uninstallSubkey) -eq 0) 'P013_UNINSTALL_HKLM_REGISTRATION_PRESENT'

    $sentinelPath = Join-Path $roamingRoot 'p013-uninstall-preservation.txt'
    Assert-P013 (-not (Test-Path -LiteralPath $sentinelPath)) 'P013_UNINSTALL_SENTINEL_ALREADY_EXISTS'
    $sentinel = [Text.Encoding]::UTF8.GetBytes('SYNVEIL P013 non-secret uninstall preservation sentinel')
    $configHashBefore = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllBytes($sentinelPath,$sentinel)
    $sentinelHash = (Get-FileHash -LiteralPath $sentinelPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $uninstallerProcess = Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART') -PassThru
    Assert-P013 (-not [SynveilP013UninstallNative]::ProcessIsElevated([uint32]$uninstallerProcess.Id)) 'P013_UNINSTALLER_ELEVATED'
    $checks.uninstaller_non_elevated = 'PASS'
    if (-not $uninstallerProcess.WaitForExit(60000)) {
        try { $uninstallerProcess.Kill() } catch { }
        throw 'P013_UNINSTALL_TIMEOUT'
    }
    Assert-P013 ($uninstallerProcess.ExitCode -eq 0) 'P013_UNINSTALL_FAILURE'
    $removeDeadline = [DateTimeOffset]::UtcNow.AddSeconds(20)
    while ((Test-Path -LiteralPath $runtimeRoot) -and [DateTimeOffset]::UtcNow -lt $removeDeadline) { Start-Sleep -Milliseconds 100 }
    $remainingTask = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction SilentlyContinue
    Assert-P013 (-not (Test-Path -LiteralPath $runtimeRoot) -and -not (Test-Path -LiteralPath $uninstallKey) -and
        (Get-HklmRegistrationCount $uninstallSubkey) -eq 0 -and $null -eq $remainingTask -and
        @((Get-Service -Name '*Synveil*' -ErrorAction SilentlyContinue)).Count -eq 0) 'P013_UNINSTALL_PACKAGE_REMAINS'
    $checks.package_removed = 'PASS'
    Assert-P013 ((Test-Path -LiteralPath $sentinelPath -PathType Leaf) -and
        (Get-FileHash -LiteralPath $sentinelPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $sentinelHash -and
        (Test-Path -LiteralPath $configPath -PathType Leaf) -and
        (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $configHashBefore) 'P013_UNINSTALL_STATE_NOT_PRESERVED'
    $checks.uninstall_preservation = 'PASS'
    Assert-P013 (@(Get-OwnedClientProcesses $clientPath $sid).Count -eq 0 -and
        @(Get-CimInstance Win32_Process -Filter "Name='synveil-desktop.exe'" | Where-Object { $_.ExecutablePath -ieq $desktopPath }).Count -eq 0) 'P013_UNINSTALL_PRODUCT_PROCESS_REMAINS'
    $checks.process_absent = 'PASS'
    $result = 'PASS'
} catch {
    $message = [string]$_.Exception.Message
    $result = if ($message -match '^P013_BLOCKED_BY_ENVIRONMENT') { 'BLOCKED_BY_ENVIRONMENT' } else { 'FAIL' }
    $errorCode = if ($message -match '^P013_[A-Z0-9_]+') { $Matches[0] } else { 'P013_UNINSTALL_ACCEPTANCE_FAILURE' }
    if ($result -eq 'BLOCKED_BY_ENVIRONMENT') {
        foreach ($name in @($checks.Keys)) { if ($checks[$name] -eq 'NOT_RUN') { $checks[$name] = 'BLOCKED_BY_ENVIRONMENT' } }
    } else {
        foreach ($name in @($checks.Keys)) { if ($checks[$name] -eq 'NOT_RUN') { $checks[$name] = 'FAIL' } }
    }
} finally {
    if ($probeRoot -and (Test-Path -LiteralPath $probeRoot)) {
        try {
            Remove-Item -LiteralPath $probeRoot -Recurse -Force -ErrorAction Stop
            if (Test-Path -LiteralPath $probeRoot) { throw 'P013_UNINSTALL_PROBE_TEMP_CLEANUP_FAILURE' }
            $checks.probe_temp_cleanup = 'PASS'
        } catch { $checks.probe_temp_cleanup = 'FAIL'; $result = 'FAIL'; $errorCode = 'P013_UNINSTALL_PROBE_TEMP_CLEANUP_FAILURE' }
    } elseif ($null -eq $probeRoot) { $checks.probe_temp_cleanup = 'PASS' }
}

$harnessBinding = @(
    'scripts/test-windows-installed-uninstall.ps1:' + (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant(),
    'crates/client/examples/p013_windows_control_probe.rs:' + (Get-FileHash -LiteralPath (Join-Path $repoRoot 'crates/client/examples/p013_windows_control_probe.rs') -Algorithm SHA256).Hash.ToLowerInvariant()
) -join "`n"
$harnessHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($harnessBinding))).ToLowerInvariant()
$record = [ordered]@{
    schema_version=1;result=$result;execution_id='p013-uninstall-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ');source_commit=$ExpectedSourceCommit
    harness_source_commit=$ExpectedHarnessCommit
    candidate=[ordered]@{filename='SynveilSetup.exe';sha256=$setupHash;size_bytes=$setupSize;source_commit=$ExpectedSourceCommit;release_manifest_sha256=$manifestHash}
    environment=[ordered]@{edition=if($os){[string]$os.Caption}else{'unavailable'};build=if($os){[int]$os.BuildNumber}else{0};architecture='x86_64';harness_revision_sha256=$harnessHash;operator_approved=($OperatorApproval -ceq $approvalText);qualification=if($checks.interactive_standard_user -ceq 'PASS'){'qualified_isolated_native'}else{'unsupported'}}
    privilege=[ordered]@{category=if($checks.interactive_standard_user -ceq 'PASS'){'standard_user'}else{'unknown'};elevated=if($null -ne $elevated){[bool]$elevated}else{$true}}
    session=[ordered]@{classification=if($checks.interactive_standard_user -ceq 'PASS'){'genuine_interactive'}else{'unsupported'};process_session_id=$sessionId;user_sid_sha256=$sidHash}
    checks=$checks;cleanup=[ordered]@{status='NOT_RUN';independently_verified=$false}
    redaction=[ordered]@{user_sid_sha256=$sidHash;user_names_recorded=$false;user_paths_recorded=$false;credentials_recorded=$false;raw_pipe_messages_recorded=$false}
    error_code=$errorCode
}
$output = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-uninstall-preservation-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
$record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $output -Encoding utf8
Write-Host "P013 installed uninstall preservation: $result"
Write-Host "Evidence: $output"
if ($result -eq 'PASS') { exit 0 }
if ($result -eq 'BLOCKED_BY_ENVIRONMENT') { exit 2 }
exit 1
