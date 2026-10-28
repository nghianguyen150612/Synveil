[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [Parameter(Mandatory=$true)][string]$ReleaseManifest,
    [Parameter(Mandatory=$true)][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][string]$ExpectedSetupSha256,
    [Parameter(Mandatory=$true)][string]$ExpectedManifestSha256,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [Parameter(Mandatory=$true)][long]$ExpectedSetupSizeBytes
)

$ErrorActionPreference = 'Stop'
$psVersion = $PSVersionTable.PSVersion
if ($psVersion.Major -lt 7) { throw 'P013_REQUIRES_POWERSHELL_7' }
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
if ($OperatorApproval -cne $approvalText) {
    throw 'P013_OPERATOR_APPROVAL_REQUIRED'
}
if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') {
    throw 'P013_BLOCKED_BY_ENVIRONMENT: hosted or CI runners cannot qualify interactive acceptance'
}
if ($ExpectedSourceCommit -notmatch '^[0-9a-f]{40}$' -or
    $ExpectedSetupSha256 -notmatch '^[0-9a-f]{64}$' -or
    $ExpectedManifestSha256 -notmatch '^[0-9a-f]{64}$') {
    throw 'P013_CANDIDATE_IDENTITY_INVALID'
}

# This gate intentionally does not create a user, start Setup, alter Task
# Scheduler state, or configure a machine. Provisioning and product execution
# are separate operator-controlled steps on an isolated disposable VM.
$setupPath = (Resolve-Path -LiteralPath $Setup).Path
$manifestPath = (Resolve-Path -LiteralPath $ReleaseManifest).Path
$setupItem = Get-Item -LiteralPath $setupPath
$setupHash = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash.ToLowerInvariant()
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($setupHash -cne $ExpectedSetupSha256 -or $manifestHash -cne $ExpectedManifestSha256) {
    throw 'P013_CANDIDATE_IDENTITY_MISMATCH'
}
if ($ExpectedSetupSizeBytes -ne $setupItem.Length) {
    throw 'P013_CANDIDATE_SIZE_MISMATCH'
}
$authenticode = Get-AuthenticodeSignature -FilePath $setupPath
if ($authenticode.Status -ne 'NotSigned') {
    throw 'P013_SIGNATURE_POLICY_UNAVAILABLE: harness input must be the existing unsigned P028 candidate'
}

$os = Get-CimInstance Win32_OperatingSystem
$arch = (Get-CimInstance Win32_ComputerSystem).SystemType
$isWindows11 = [int]$os.BuildNumber -ge 22000 -and $os.Caption -match 'Windows 11'
$isX64 = $env:PROCESSOR_ARCHITEW6432 -eq 'AMD64' -or $env:PROCESSOR_ARCHITECTURE -eq 'AMD64'

$nativeType = @'
using System;
using System.Runtime.InteropServices;
public static class SynveilP013SessionNative {
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool GetUserObjectInformation(IntPtr handle, int index, System.Text.StringBuilder value, int length, out int needed);
  [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
  [DllImport("kernel32.dll", SetLastError=true)] public static extern bool ProcessIdToSessionId(uint processId, out uint sessionId);
  [DllImport("wtsapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool WTSQuerySessionInformation(IntPtr server, uint sessionId, int infoClass, out IntPtr buffer, out uint bytes);
  [DllImport("wtsapi32.dll")] public static extern void WTSFreeMemory(IntPtr buffer);
  [DllImport("advapi32.dll", SetLastError=true)] public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError=true)] public static extern bool GetTokenInformation(IntPtr token, int infoClass, out int value, int length, out int needed);
  [DllImport("kernel32.dll")] public static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
  public static bool IsElevated() {
    IntPtr token; int value, needed;
    if (!OpenProcessToken(GetCurrentProcess(), 0x0008, out token)) throw new System.ComponentModel.Win32Exception();
    try { if (!GetTokenInformation(token, 20, out value, sizeof(int), out needed)) throw new System.ComponentModel.Win32Exception(); return value != 0; }
    finally { CloseHandle(token); }
  }
}
'@
if (-not ('SynveilP013SessionNative' -as [type])) { Add-Type -TypeDefinition $nativeType }

$currentProcessSession = [uint32]0
$processSessionOk = [SynveilP013SessionNative]::ProcessIdToSessionId([uint32]$PID, [ref]$currentProcessSession)
$consoleSession = [SynveilP013SessionNative]::WTSGetActiveConsoleSessionId()
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$administrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$desktopHandle = [SynveilP013SessionNative]::OpenInputDesktop(0, $false, 0x0001)
$desktopName = ''
if ($desktopHandle -ne [IntPtr]::Zero) {
    try {
        $desktopNameBuilder = [Text.StringBuilder]::new(128)
        $needed = 0
        if ([SynveilP013SessionNative]::GetUserObjectInformation($desktopHandle, 2, $desktopNameBuilder, $desktopNameBuilder.Capacity, [ref]$needed)) {
            $desktopName = $desktopNameBuilder.ToString()
        }
    } finally { [void][SynveilP013SessionNative]::CloseDesktop($desktopHandle) }
}
$wtsConnected = $false
$wtsBuffer = [IntPtr]::Zero
$wtsBytes = [uint32]0
if ($processSessionOk -and [SynveilP013SessionNative]::WTSQuerySessionInformation([IntPtr]::Zero, $currentProcessSession, 8, [ref]$wtsBuffer, [ref]$wtsBytes)) {
    try { $wtsConnected = ([Runtime.InteropServices.Marshal]::ReadInt32($wtsBuffer) -eq 0) }
    finally { [SynveilP013SessionNative]::WTSFreeMemory($wtsBuffer) }
}
$wtsBuffer = [IntPtr]::Zero
$wtsBytes = [uint32]0
$wtsUserName = $null
if ($processSessionOk -and [SynveilP013SessionNative]::WTSQuerySessionInformation([IntPtr]::Zero, $currentProcessSession, 5, [ref]$wtsBuffer, [ref]$wtsBytes)) {
    try { $wtsUserName = [Runtime.InteropServices.Marshal]::PtrToStringUni($wtsBuffer) }
    finally { [SynveilP013SessionNative]::WTSFreeMemory($wtsBuffer) }
}
$wtsBuffer = [IntPtr]::Zero
$wtsBytes = [uint32]0
$wtsDomainName = $null
if ($processSessionOk -and [SynveilP013SessionNative]::WTSQuerySessionInformation([IntPtr]::Zero, $currentProcessSession, 7, [ref]$wtsBuffer, [ref]$wtsBytes)) {
    try { $wtsDomainName = [Runtime.InteropServices.Marshal]::PtrToStringUni($wtsBuffer) }
    finally { [SynveilP013SessionNative]::WTSFreeMemory($wtsBuffer) }
}
$wtsUserSid = $null
if (-not [string]::IsNullOrWhiteSpace($wtsUserName)) {
    try {
        $qualifiedWtsName = if ([string]::IsNullOrWhiteSpace($wtsDomainName)) { $wtsUserName } else { "$wtsDomainName\$wtsUserName" }
        $wtsUserSid = ([Security.Principal.NTAccount]::new($qualifiedWtsName)).Translate([Security.Principal.SecurityIdentifier]).Value
    } catch { $wtsUserSid = $null }
}
$wtsSessionHasUser = -not [string]::IsNullOrWhiteSpace($wtsUserSid)
$processMatchesWtsUser = $wtsSessionHasUser -and $identity.User.Value -ceq $wtsUserSid
$inputDesktopOpen = $desktopName -ceq 'Default'
$activeSessionMatch = $processSessionOk -and $currentProcessSession -ne 0xffffffff -and $currentProcessSession -eq $consoleSession
$interactive = $inputDesktopOpen -and $wtsConnected -and $currentProcessSession -ne 0 -and $wtsSessionHasUser -and $processMatchesWtsUser
$elevated = [SynveilP013SessionNative]::IsElevated()
$sidHasher = [Security.Cryptography.SHA256]::Create()
try { $sidDigest = [Convert]::ToHexString($sidHasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant() }
finally { $sidHasher.Dispose() }
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$hasAdminGroup = @($identity.Groups | Where-Object { $_.Value -eq $adminSid.Value }).Count -gt 0

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$artifactRecords = @($manifest.artifacts | Where-Object {
    $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe'
})
$manifestCandidateMatch = $manifest.schema_version -eq 1 -and $manifest.product -ceq 'Synveil' -and
    $manifest.source_commit -ceq $ExpectedSourceCommit -and
    $artifactRecords.Count -eq 1 -and $artifactRecords[0].sha256 -ceq $setupHash -and
    [long]$artifactRecords[0].size_bytes -eq $setupItem.Length

$checkNames = @(
    'candidate_identity','architecture','windows_build_policy','standard_user_token',
    'interactive_session','false_interactive_claim_rejection','real_logon','logon_trigger',
    'setup_process_identity','pipe_endpoint_discovery','installed_binary_identity',
    'installed_process_identity','pipe_protocol_v1','pipe_ping','pipe_wrong_user',
    'pipe_unsupported_version','pipe_oversized_payload','pipe_malformed_message',
    'pipe_expired_endpoint','pipe_server_termination','pipe_duplicate_connection',
    'pipe_stale_profile','pipe_restart','pipe_unauthorized_operation',
    'pipe_timeout_cancellation','no_tcp_fallback','setup_gui_launch','gui_welcome',
    'gui_user_options','gui_installation','gui_completion','installed_app_startup',
    'first_run_presentation','no_unexpected_uac','consistent_runtime_identity',
    'application_shutdown','logon_expected_sid','logon_no_elevated_token',
    'logon_startup_state','logon_no_duplicate_processes','logon_logout_shutdown',
    'install_root','uninstall_preservation','cleanup','timeout_cancellation',
    'evidence_redaction','no_wizard_execution_activation'
)
$checks = [ordered]@{}
foreach ($name in $checkNames) { $checks[$name] = 'NOT_RUN' }
$checks.candidate_identity = if ($manifestCandidateMatch) { 'PASS' } else { 'FAIL' }
$checks.architecture = if ($isX64) { 'PASS' } else { 'FAIL' }
$checks.windows_build_policy = if ($isWindows11) { 'PASS' } else { 'FAIL' }
$checks.standard_user_token = if (-not $administrator -and -not $hasAdminGroup -and -not $elevated) { 'PASS' } else { 'FAIL' }
$checks.interactive_session = if ($interactive) { 'PASS' } else { 'BLOCKED_BY_ENVIRONMENT' }
$checks.false_interactive_claim_rejection = 'PASS'
$checks.evidence_redaction = 'PASS'
$checks.no_wizard_execution_activation = 'PASS'

$sessionClass = if ($interactive) { 'genuine_interactive' } elseif ($currentProcessSession -eq 0) { 'service' } elseif ($wtsSessionHasUser -and -not $processMatchesWtsUser) { 'alternate_credentials_noninteractive' } else { 'unsupported' }
$interactiveStatus = if ($interactive) { 'PASS' } else { 'BLOCKED_BY_ENVIRONMENT' }
$unrunRequired = @($checks.GetEnumerator() | Where-Object { $_.Value -in @('NOT_RUN','INCOMPLETE') }).Count -gt 0
$result = if (-not $isWindows11 -or -not $isX64 -or $administrator -or $hasAdminGroup -or $elevated) { 'FAIL' }
    elseif (-not $interactive) { 'BLOCKED_BY_ENVIRONMENT' }
    elseif ($unrunRequired) { 'INCOMPLETE' } else { 'PASS' }

$manifestSha = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$harnessSha = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
$record = [ordered]@{
    schema_version = 1
    result = $result
    execution_id = 'p013-' + [Guid]::NewGuid().ToString('N')
    executed_at_utc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    source_commit = $ExpectedSourceCommit
    candidate = [ordered]@{
        filename = 'SynveilSetup.exe'; sha256 = $setupHash; size_bytes = $setupItem.Length
        source_commit = [string]$manifest.source_commit; release_manifest_sha256 = $manifestSha
        expected_sha256 = $ExpectedSetupSha256; expected_size_bytes = [long]$ExpectedSetupSizeBytes
        expected_release_manifest_sha256 = $ExpectedManifestSha256
        signature_status = 'UNSIGNED_FIXTURE'
    }
    environment = [ordered]@{
        edition = [string]$os.Caption; build = [int]$os.BuildNumber
        architecture = if ($isX64) { 'x86_64' } else { [string]$arch }
        harness_revision_sha256 = $harnessSha
        operator_approved = $true
        qualification = if ($env:GITHUB_ACTIONS -eq 'true') { 'github_hosted_service' } elseif ($isWindows11 -and $isX64 -and $interactive -and -not $administrator -and -not $hasAdminGroup -and -not $elevated) { 'qualified_isolated_native' } else { 'unsupported' }
    }
    privilege = [ordered]@{
        category = if ($administrator -or $hasAdminGroup -or $elevated) { 'administrator' } else { 'standard_user' }
        elevated = [bool]$elevated
    }
    session = [ordered]@{
        classification = $sessionClass; interactive_status = $interactiveStatus
        new_logon_status = 'NOT_RUN'; trigger_status = 'NOT_RUN'
        native_observations = [ordered]@{
            input_desktop_open = [bool]$inputDesktopOpen
            wts_session_active = [bool]$wtsConnected
            active_console_session_match = [bool]$activeSessionMatch
            wts_session_has_user = [bool]$wtsSessionHasUser
            process_user_matches_wts_identity = [bool]$processMatchesWtsUser
        }
        logon_observations = [ordered]@{
            security_4624_matches_current_logon_luid = $false
            logon_type_interactive = $false; task_logon_trigger_event = $false
        }
    }
    checks = $checks
    cleanup = [ordered]@{ status = 'NOT_RUN'; independently_verified = $false }
    redaction = [ordered]@{ no_credentials_or_user_paths = $true; raw_pipe_payloads_recorded = $false }
    identity_binding = [ordered]@{ user_sid_sha256 = $sidDigest; process_session_id = [uint32]$currentProcessSession }
}

# The JSON contract intentionally forbids publishing account names, raw SIDs,
# paths, credentials, and pipe bytes. Retain only a one-way SID commitment.
$outputPath = Join-Path ([IO.Path]::GetFullPath($EvidenceDirectory)) 'windows-native-environment-v1.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $outputPath) -Force | Out-Null
$record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $outputPath -Encoding utf8
Write-Host "P013 environment result: $result"
Write-Host "Evidence: $outputPath"
if ($result -ne 'PASS') { exit 2 }
