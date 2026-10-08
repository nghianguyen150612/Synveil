param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$RepositoryRoot,
    [Parameter(Mandatory=$true)][string]$EvidencePath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceCommit,
    [Parameter(Mandatory=$true)][string]$ExpectedSid
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-known-folders.ps1')
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if ($identity.User.Value -cne $ExpectedSid -or $isAdmin) {
    throw 'STANDARD_USER_IDENTITY_FAILURE: child token does not match the disposable non-administrator account'
}
$profileRoot = [SynveilKnownFolders]::CurrentUserProfile()
$profileEnvMatches = $false
if (![string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
    $profileEnvMatches = [string]::Equals([IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\'), $profileRoot.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
}
$localAppDataEnvMatches = $false
if (![string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    $localAppDataEnvMatches = [string]::Equals([IO.Path]::GetFullPath($env:LOCALAPPDATA).TrimEnd('\'), (Join-Path $profileRoot 'AppData\Local').TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
}
Write-Output "STANDARD_USER_PREFLIGHT: expected SID matched; administrator=false; USERPROFILE_matches_token_profile=$profileEnvMatches; LOCALAPPDATA_matches_token_profile=$localAppDataEnvMatches"
$localAppData = Get-WindowsKnownFolderPath LocalApplicationData
$appId = '{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
$uninstallSubkey = "Software\Microsoft\Windows\CurrentVersion\Uninstall\$appId"
$root = Join-Path $localAppData 'Programs\Synveil'
$startMenu = Join-Path (Get-WindowsKnownFolderPath Programs) 'Synveil.lnk'
$desktop = Join-Path (Get-WindowsKnownFolderPath DesktopDirectory) 'Synveil.lnk'
$commonStartMenu = Join-Path (Get-WindowsKnownFolderPath CommonPrograms) 'Synveil.lnk'
$publicDesktop = Join-Path (Get-WindowsKnownFolderPath CommonDesktopDirectory) 'Synveil.lnk'
$logRoot = Join-Path $localAppData 'Synveil\installer'
$sentinel = Join-Path $localAppData 'Synveil\state-sentinel\p025.txt'

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class SynveilTokenEvidence {
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr p, UInt32 a, out IntPtr t);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr h);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr t, int c, IntPtr b, int l, out int n);
  static byte[] Info(IntPtr token, int kind) {
    int n; GetTokenInformation(token, kind, IntPtr.Zero, 0, out n);
    IntPtr b=Marshal.AllocHGlobal(n); try { if(!GetTokenInformation(token,kind,b,n,out n)) throw new Win32Exception(); byte[] x=new byte[n]; Marshal.Copy(b,x,0,n); return x; } finally { Marshal.FreeHGlobal(b); }
  }
  static string[] ForProcess(IntPtr process) {
    IntPtr t; if(!OpenProcessToken(process, 8, out t)) throw new Win32Exception();
    try {
      byte[] e=Info(t,20); int elevation=BitConverter.ToInt32(e,0);
      byte[] et=Info(t,18); int elevationType=BitConverter.ToInt32(et,0);
      int n; GetTokenInformation(t,25,IntPtr.Zero,0,out n); IntPtr b=Marshal.AllocHGlobal(n);
      try { if(!GetTokenInformation(t,25,b,n,out n)) throw new Win32Exception(); IntPtr sid=Marshal.ReadIntPtr(b); string integrity=new SecurityIdentifier(sid).Value; return new[]{elevation.ToString(),elevationType.ToString(),integrity}; }
      finally { Marshal.FreeHGlobal(b); }
    } finally { CloseHandle(t); }
  }
  public static string[] Current() { return ForProcess(GetCurrentProcess()); }
  public static string[] ForProcessHandle(IntPtr process) { return ForProcess(process); }
}
'@

function Assert-True([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
$script:observedTokens = @{}
function Invoke-Bounded([string]$File, [string[]]$Arguments, [string]$WorkingDirectory, [int]$ExpectedExit, [string]$Name, [string]$Stdout='', [string]$Stderr='') {
    $options = @{ FilePath=$File; ArgumentList=$Arguments; WorkingDirectory=$WorkingDirectory; PassThru=$true }
    if ($Stdout) { $options.RedirectStandardOutput=$Stdout }
    if ($Stderr) { $options.RedirectStandardError=$Stderr }
    $p = Start-Process @options
    $processToken = [SynveilTokenEvidence]::ForProcessHandle($p.Handle)
    Assert-True ($processToken[0] -eq '0' -and $processToken[2] -notin @('S-1-16-12288','S-1-16-16384')) "PER_USER_TOKEN_FAILURE: $Name obtained an elevated/high token"
    $script:observedTokens[$Name] = [ordered]@{ elevated=$false; elevation_type=[int]$processToken[1]; integrity_sid=$processToken[2] }
    if (!$p.WaitForExit(30000)) { $p.Kill(); throw "$Name timed out" }
    if ($p.ExitCode -ne $ExpectedExit) { throw "$Name exit $($p.ExitCode), expected $ExpectedExit" }
}
function Get-HklmRegistrationCount {
    $count = 0
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,$view)
        try { $key = $base.OpenSubKey($uninstallSubkey); if ($null -ne $key) { $count++; $key.Dispose() } } finally { $base.Dispose() }
    }
    return $count
}
function Get-SynveilServiceCount { return @((Get-Service -Name '*Synveil*' -ErrorAction SilentlyContinue)).Count }
function Get-SynveilTaskCount { return @((Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object { $_.TaskName -match 'Synveil' -or $_.TaskPath -match 'Synveil' })).Count }

$token = [SynveilTokenEvidence]::Current()
Assert-True (!$isAdmin) 'PER_USER_TOKEN_FAILURE: test account belongs to Administrators'
Assert-True ($token[0] -eq '0') 'PER_USER_TOKEN_FAILURE: harness token is elevated'
Assert-True ($token[2] -notin @('S-1-16-12288','S-1-16-16384')) 'PER_USER_TOKEN_FAILURE: high/system integrity token'

$machinePathBefore = [Environment]::GetEnvironmentVariable('Path','Machine')
$userPathBefore = [Environment]::GetEnvironmentVariable('Path','User')
$servicesBefore = Get-SynveilServiceCount
$tasksBefore = Get-SynveilTaskCount
$hklmBefore = Get-HklmRegistrationCount
Assert-True ($hklmBefore -eq 0) 'PER_USER_REGISTRY_FAILURE: pre-existing HKLM product identity conflicts with per-user mode'
New-Item $logRoot -ItemType Directory -Force | Out-Null
New-Item (Split-Path $sentinel) -ItemType Directory -Force | Out-Null
Set-Content -LiteralPath $sentinel -Value 'P025 synthetic non-secret state sentinel'
$setupHash = (Get-FileHash -LiteralPath $Setup -Algorithm SHA256).Hash.ToLowerInvariant()
$unrelatedCwd = Join-Path $logRoot 'unrelated-cwd'
New-Item $unrelatedCwd -ItemType Directory -Force | Out-Null

Invoke-Bounded $Setup @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/STARTUP=0','/DESKTOPICON=0','/LAUNCH=0',('/LOG=' + (Join-Path $logRoot 'install.log'))) $unrelatedCwd 0 'installer'
Assert-True (Test-Path $root -PathType Container) 'PER_USER_INSTALL_FAILURE: LocalAppData package root missing'
Assert-True (Test-Path $startMenu -PathType Leaf) 'PER_USER_INSTALL_FAILURE: current-user Start Menu shortcut missing'
Assert-True (!(Test-Path $desktop)) 'PER_USER_INSTALL_FAILURE: DESKTOPICON=0 created a desktop shortcut'
Assert-True (!(Test-Path $commonStartMenu) -and !(Test-Path $publicDesktop)) 'PER_USER_INSTALL_FAILURE: common shortcut created'
Assert-True ((Get-HklmRegistrationCount) -eq $hklmBefore) 'PER_USER_REGISTRY_FAILURE: HKLM uninstall registration changed'
$hkcu = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallSubkey)
Assert-True ($null -ne $hkcu) 'PER_USER_REGISTRY_FAILURE: HKCU uninstall registration missing'
$uninstaller = $hkcu.GetValue('UninstallString').Trim('"'); $hkcu.Dispose()
Assert-True (Test-Path $uninstaller -PathType Leaf) 'PER_USER_REGISTRY_FAILURE: registered uninstaller missing'

& (Join-Path $RepositoryRoot 'scripts\test-windows-installed-runtime.ps1') -RuntimeRoot $root -Manifest (Join-Path $root 'SYNVEIL-MANIFEST.txt')
# The PowerShell verifier throws on failure under Stop. LASTEXITCODE describes
# native executables, not this script invocation, and can be unset here.
$acl = Get-Acl -LiteralPath $root
$broadWrite = @($acl.Access | Where-Object {
    $_.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and
    $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -in @('S-1-1-0','S-1-5-11','S-1-5-32-545') -and
    ($_.FileSystemRights -band ([Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Modify -bor [Security.AccessControl.FileSystemRights]::FullControl))
})
Assert-True ($broadWrite.Count -eq 0) 'PER_USER_ACL_FAILURE: broad local-user principal can write package root'

foreach ($name in @('QT_ROOT_DIR','QT_PLUGIN_PATH','QML2_IMPORT_PATH','QML_IMPORT_PATH')) { Remove-Item "Env:$name" -ErrorAction SilentlyContinue }
$env:QT_QPA_PLATFORM='offscreen'; $env:QT_DEBUG_PLUGINS='1'; $env:PATH="$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\Wbem"
$desktopOut=Join-Path $logRoot 'desktop.stdout.log'; $desktopErr=Join-Path $logRoot 'desktop.stderr.log'
Invoke-Bounded (Join-Path $root 'synveil-desktop.exe') @('--qml-smoke-test') $unrelatedCwd 0 'desktop smoke' $desktopOut $desktopErr
$diagnostics = Get-Content $desktopErr -Raw
Assert-True ($diagnostics -notmatch [regex]::Escape($RepositoryRoot) -and $diagnostics -notmatch 'hostedtoolcache.*Qt') 'PER_USER_RUNTIME_FAILURE: desktop used checkout/Qt SDK path'
$env:QT_DEBUG_PLUGINS='0'
Invoke-Bounded (Join-Path $root 'synveil-client.exe') @() $unrelatedCwd 78 'client probe' (Join-Path $logRoot 'client.stdout.log') (Join-Path $logRoot 'client.stderr.log')
Assert-True ([Environment]::GetEnvironmentVariable('Path','Machine') -ceq $machinePathBefore) 'PER_USER_MUTATION_FAILURE: machine PATH changed'
Assert-True ([Environment]::GetEnvironmentVariable('Path','User') -ceq $userPathBefore) 'PER_USER_MUTATION_FAILURE: user PATH changed'
Assert-True ((Get-SynveilServiceCount) -eq $servicesBefore) 'PER_USER_MUTATION_FAILURE: Synveil service created'
Assert-True ((Get-SynveilTaskCount) -eq $tasksBefore) 'PER_USER_MUTATION_FAILURE: Synveil scheduled task created'

Invoke-Bounded $uninstaller @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG=' + (Join-Path $logRoot 'uninstall.log'))) $unrelatedCwd 0 'uninstaller'
Assert-True (!(Test-Path $root)) 'PER_USER_UNINSTALL_FAILURE: package root remains'
Assert-True (!(Test-Path $startMenu)) 'PER_USER_UNINSTALL_FAILURE: Start Menu shortcut remains'
Assert-True ($null -eq [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallSubkey)) 'PER_USER_UNINSTALL_FAILURE: HKCU registration remains'
Assert-True (Test-Path $sentinel -PathType Leaf) 'PER_USER_UNINSTALL_FAILURE: external state sentinel removed'

Invoke-Bounded $Setup @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/STARTUP=0','/DESKTOPICON=1','/LAUNCH=0',('/LOG=' + (Join-Path $logRoot 'reinstall.log'))) $unrelatedCwd 0 'reinstaller'
Assert-True (Test-Path $desktop -PathType Leaf) 'PER_USER_OPTION_FAILURE: DESKTOPICON=1 shortcut missing'
$hkcu = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallSubkey); Assert-True ($null -ne $hkcu) 'PER_USER_REINSTALL_FAILURE: HKCU registration missing'
$uninstaller = $hkcu.GetValue('UninstallString').Trim('"'); $hkcu.Dispose()
Invoke-Bounded $uninstaller @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG=' + (Join-Path $logRoot 'uninstall-reinstall.log'))) $unrelatedCwd 0 'second uninstaller'
Assert-True (!(Test-Path $desktop) -and (Test-Path $sentinel)) 'PER_USER_UNINSTALL_FAILURE: option cleanup/state preservation failed'

$os = Get-CimInstance Win32_OperatingSystem
$evidence = [ordered]@{
    schema_version=1; source_commit=$SourceCommit; artifact_sha256=$setupHash
    windows_version=$os.Version; windows_build=$os.BuildNumber; architecture=$env:PROCESSOR_ARCHITECTURE
    test_account_category='synthetic_standard_user'; administrator_member=$isAdmin
    installer_elevated=$script:observedTokens['installer'].elevated; installer_token=$script:observedTokens['installer']
    harness_token_elevation_type=[int]$token[1]; harness_integrity_sid=$token[2]
    install_root='%LOCALAPPDATA%\Programs\Synveil'; hkcu_registration='present_then_removed'
    hklm_registration='absent_before_and_after'; current_user_shortcuts='start_menu_and_option_verified'
    common_shortcuts='absent'; package_acl='no_broad_user_write'; desktop_smoke='pass'; client_smoke='pass'
    machine_path_unchanged=$true; user_path_unchanged=$true; service_created=$false; scheduled_task_created=$false
    uninstall='pass'; reinstall='pass'; state_preservation='pass'; cleanup='pending_controller'
}
New-Item (Split-Path $EvidencePath) -ItemType Directory -Force | Out-Null
$evidence | ConvertTo-Json | Set-Content -LiteralPath $EvidencePath -Encoding utf8
Write-Host 'Windows standard-user installation: PASS'
