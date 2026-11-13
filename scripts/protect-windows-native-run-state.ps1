[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Initialize','Verify','Advance')][string]$Action,
    [Parameter(Mandatory=$true)][string]$ApprovedRoot,
    [string]$RunId,
    [Parameter(Mandatory=$true)][ValidatePattern('^S-1-5-21-(?:[0-9]+-){3}[0-9]+$')][string]$SubjectSid,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$SourceCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$HarnessCommit,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$CandidateSha256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory=$true)][DateTimeOffset]$ExpiresUtc,
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [ValidateSet('PREFLIGHT','CANDIDATE_VERIFIED','ENVIRONMENT_QUALIFIED','INSTALLED','GUI_VERIFIED','IPC_VERIFIED','STARTUP_CONFIGURED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED','LIFECYCLE_VERIFIED','CLEANUP_VERIFIED','EVIDENCE_FINALIZED')][string]$Phase,
    [string]$EvidenceFile
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$systemSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$subject = [Security.Principal.SecurityIdentifier]::new($SubjectSid)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($OperatorApproval -cne $approvalText) { throw 'P028_OPERATOR_APPROVAL_REQUIRED' }
if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'P028_REQUIRES_POWERSHELL_7' }
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'P028_TRUSTED_CONTROLLER_REQUIRES_ADMIN' }
$nativeElevation = @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class P028TokenElevationCheck {
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, UInt32 access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int infoClass, out UInt32 info, UInt32 length, out UInt32 returned);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
    public static bool IsElevated() {
        IntPtr token;
        if (!OpenProcessToken(Process.GetCurrentProcess().Handle, 0x0008, out token)) throw new System.ComponentModel.Win32Exception();
        try { UInt32 value, returned; if (!GetTokenInformation(token, 20, out value, 4, out returned)) throw new System.ComponentModel.Win32Exception(); return value == 1; }
        finally { CloseHandle(token); }
    }
}
'@
if (-not ('P028TokenElevationCheck' -as [type])) { Add-Type -TypeDefinition $nativeElevation }
if (-not [P028TokenElevationCheck]::IsElevated()) { throw 'P028_TRUSTED_CONTROLLER_NOT_ELEVATED' }
if ($subject -eq $identity.User -or $subject -eq $adminSid -or $subject -eq $systemSid) { throw 'P028_SUBJECT_IDENTITY_INVALID' }
$subjectAccount = $subject.Translate([Security.Principal.NTAccount]).Value
if ($subjectAccount -notmatch ('^' + [regex]::Escape($env:COMPUTERNAME) + '\\synveil_p013_[0-9a-f]{8}$')) { throw 'P028_SUBJECT_NOT_DISPOSABLE_LOCAL_USER' }
$administrators = @(Get-LocalGroupMember -SID $adminSid | ForEach-Object { try { $_.SID.Value } catch { $_.Name } })
if ($administrators -contains $subject.Value) { throw 'P028_SUBJECT_IS_ADMINISTRATOR' }
$windows = Get-CimInstance Win32_OperatingSystem
if ($windows.Caption -notmatch 'Windows 11' -or [int]$windows.BuildNumber -lt 22000 -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'P028_NATIVE_PLATFORM_UNSUPPORTED' }
if ($ExpiresUtc.Offset -ne [TimeSpan]::Zero -or $ExpiresUtc -le [DateTimeOffset]::UtcNow -or $ExpiresUtc -gt [DateTimeOffset]::UtcNow.AddDays(7)) { throw 'P028_RUN_DEADLINE_INVALID' }
if ($Action -eq 'Initialize') {
    if (-not [string]::IsNullOrWhiteSpace($RunId)) { throw 'P028_RUN_ID_MUST_BE_GENERATED' }
    $RunId = [Guid]::NewGuid().ToString('N')
} elseif ($RunId -notmatch '^[0-9a-f]{32}$') { throw 'P028_RUN_ID_INVALID' }

function Assert-NoReparsePath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    $drive = [IO.Path]::GetPathRoot($full)
    $current = $drive
    $tail = $full.Substring($drive.Length).Split([IO.Path]::DirectorySeparatorChar, [StringSplitOptions]::RemoveEmptyEntries)
    foreach ($part in $tail) {
        $current = Join-Path $current $part
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'P028_REPARSE_POINT_REJECTED' }
        }
    }
    return $full
}
function Get-ExpectedAcl([string]$Path, [bool]$Inbox, [bool]$IncludeSubject = $true) {
    $acl = Get-Acl -LiteralPath $Path
    if (-not $acl.AreAccessRulesProtected) { throw 'P028_INHERITED_ACL_REJECTED' }
    $owner = $acl.GetOwner([Security.Principal.SecurityIdentifier])
    if ($owner -ne $adminSid) { throw 'P028_STATE_OWNER_MISMATCH' }
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    $expected = @{}
    $expected[$adminSid.Value] = [Security.AccessControl.FileSystemRights]::FullControl
    $expected[$systemSid.Value] = [Security.AccessControl.FileSystemRights]::FullControl
    if ($IncludeSubject) { $expected[$subject.Value] = if ($Inbox) { [Security.AccessControl.FileSystemRights]::Modify } else { [Security.AccessControl.FileSystemRights]::ReadAndExecute } }
    # Allow rules constructed by .NET include Synchronize even when the named
    # rights constant omits it. Compare the resulting mask, not effective access.
    foreach ($key in @($expected.Keys)) {
        $expected[$key] = $expected[$key] -bor [Security.AccessControl.FileSystemRights]::Synchronize
    }
    foreach ($rule in $rules) {
        $sid = $rule.IdentityReference.Value
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or
            $rule.IsInherited -or
            $rule.InheritanceFlags -ne ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit) -or
            $rule.PropagationFlags -ne [Security.AccessControl.PropagationFlags]::None -or
            -not $expected.ContainsKey($sid) -or
            $rule.FileSystemRights -ne $expected[$sid]) {
            Write-Warning ("P028_ACL_RULE_DIAGNOSTIC rights={0} mask=0x{1:X} inheritance={2} propagation={3} type={4} inherited={5} expectedPrincipal={6}" -f $rule.FileSystemRights, [int]$rule.FileSystemRights, $rule.InheritanceFlags, $rule.PropagationFlags, $rule.AccessControlType, $rule.IsInherited, $expected.ContainsKey($sid))
            throw 'P028_UNEXPECTED_ACL_RULE'
        }
        $expected.Remove($sid)
    }
    if ($expected.Count -ne 0) { throw 'P028_REQUIRED_ACL_MISSING' }
}
function Set-RunAcl([string]$Path, [bool]$Inbox, [bool]$IncludeSubject = $true) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner($adminSid)
    $inheritance = [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit
    $entries = @(
        [pscustomobject]@{Sid=$adminSid; Rights=[Security.AccessControl.FileSystemRights]::FullControl},
        [pscustomobject]@{Sid=$systemSid; Rights=[Security.AccessControl.FileSystemRights]::FullControl}
    )
    if ($IncludeSubject) {
        $subjectRights = if ($Inbox) { [Security.AccessControl.FileSystemRights]::Modify } else { [Security.AccessControl.FileSystemRights]::ReadAndExecute }
        $entries += [pscustomobject]@{Sid=$subject; Rights=$subjectRights}
    }
    foreach ($entry in $entries) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($entry.Sid, $entry.Rights, $inheritance,
            [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
    Get-ExpectedAcl $Path $Inbox $IncludeSubject
}

function Get-MachineDigest {
    $value = [string](Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography' -Name MachineGuid)
    if ([string]::IsNullOrWhiteSpace($value)) { throw 'P028_MACHINE_ID_UNAVAILABLE' }
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($value))).ToLowerInvariant()
}
$safeReadType = @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class P028SafeEvidenceReader {
    [StructLayout(LayoutKind.Sequential)] struct FileAttributeTagInfo { public UInt32 Attributes; public UInt32 ReparseTag; }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string name, UInt32 access, UInt32 share, IntPtr security, UInt32 creation, UInt32 flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int infoClass, out FileAttributeTagInfo info, UInt32 size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileSizeEx(SafeFileHandle handle, out long size);
    public static byte[] Read(string path, int maximum) {
        using (SafeFileHandle handle = CreateFile(path, 0x80000000, 1, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero)) {
            if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            FileAttributeTagInfo info;
            if (!GetFileInformationByHandleEx(handle, 9, out info, (UInt32)Marshal.SizeOf<FileAttributeTagInfo>())) throw new Win32Exception(Marshal.GetLastWin32Error());
            if ((info.Attributes & 0x400) != 0) throw new InvalidDataException("P028_REPARSE_POINT_REJECTED");
            long length;
            if (!GetFileSizeEx(handle, out length)) throw new Win32Exception(Marshal.GetLastWin32Error());
            if (length <= 0 || length > maximum) throw new InvalidDataException("P028_EVIDENCE_SIZE_INVALID");
            using (FileStream stream = new FileStream(handle, FileAccess.Read)) {
                byte[] bytes = new byte[(int)length]; int offset = 0;
                while (offset < bytes.Length) { int count = stream.Read(bytes, offset, bytes.Length-offset); if (count == 0) throw new EndOfStreamException("P028_EVIDENCE_CHANGED_DURING_READ"); offset += count; }
                if (stream.ReadByte() != -1) throw new IOException("P028_EVIDENCE_CHANGED_DURING_READ");
                return bytes;
            }
        }
    }
}
'@
if (-not ('P028SafeEvidenceReader' -as [type])) { Add-Type -TypeDefinition $safeReadType }
function Save-RunState($State) {
    $temporary = Join-Path $trustedPath ('.run-state-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($State | ConvertTo-Json -Depth 12) + "`n")
    if ($bytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $statePath, $true)
}
$root = Assert-NoReparsePath $ApprovedRoot
if (-not [IO.Path]::IsPathFullyQualified($root) -or $root -notmatch '^[A-Z]:\\ProgramData\\Synveil\\P028(?:\\|$)') { throw 'P028_APPROVED_ROOT_INVALID' }
if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw 'P028_APPROVED_ROOT_MISSING' }
Get-ExpectedAcl $root $false $false
$runRoot = Join-Path $root $RunId
$trustedPath = Join-Path $runRoot 'trusted'
$inboxPath = Join-Path $runRoot 'subject-inbox'
$observerInbox = Join-Path $trustedPath 'observer-inbox'
$evidenceRoot = Join-Path $trustedPath 'evidence'
$statePath = Join-Path $trustedPath 'run-state.json'
$lockPath = Join-Path $trustedPath 'coordinator.lock'

if ($Action -eq 'Initialize') {
    if (Test-Path -LiteralPath $runRoot) { throw 'P028_RUN_DIRECTORY_EXISTS' }
    [void][IO.Directory]::CreateDirectory($runRoot)
    Set-RunAcl $runRoot $false
    [void][IO.Directory]::CreateDirectory($trustedPath)
    Set-RunAcl $trustedPath $false
    [void][IO.Directory]::CreateDirectory($inboxPath)
    Set-RunAcl $inboxPath $true
    [void][IO.Directory]::CreateDirectory($observerInbox)
    Set-RunAcl $observerInbox $false $false
    [void][IO.Directory]::CreateDirectory($evidenceRoot)
    Set-RunAcl $evidenceRoot $false $false
    $template = [ordered]@{
        schema_version=1; run_id=$RunId; status='ACTIVE'; mode='WINDOWS_PROTECTED_PENDING_NATIVE_AUTHORITY'
        source_commit=$SourceCommit; harness_commit=$HarnessCommit; candidate_sha256=$CandidateSha256
        manifest_sha256=$ManifestSha256; subject_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($subject.Value))).ToLowerInvariant()
        controller_sid_sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
        machine_guid_sha256=(Get-MachineDigest); os_build=[int]$windows.BuildNumber; architecture='x86_64'; created_at_utc=[DateTimeOffset]::UtcNow.ToString('o')
        expires_at_utc=$ExpiresUtc.ToString('o'); phases=@(); finalization_digest=$null
    }
    $json = $template | ConvertTo-Json -Depth 8
    $temporary = Join-Path $trustedPath ('.run-state-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    if ($bytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $statePath)
    Get-ExpectedAcl $runRoot $false
    Get-ExpectedAcl $trustedPath $false
    Get-ExpectedAcl $inboxPath $true
    Get-ExpectedAcl $observerInbox $false $false
    Get-ExpectedAcl $evidenceRoot $false $false
    Write-Output "P028_PROTECTED_RUN_CREATED $RunId"
    exit 0
}

if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) { throw 'P028_RUN_STATE_MISSING' }
[void](Assert-NoReparsePath $runRoot)
[void](Assert-NoReparsePath $trustedPath)
[void](Assert-NoReparsePath $inboxPath)
[void](Assert-NoReparsePath $observerInbox)
[void](Assert-NoReparsePath $evidenceRoot)
Get-ExpectedAcl $runRoot $false
Get-ExpectedAcl $trustedPath $false
Get-ExpectedAcl $inboxPath $true
Get-ExpectedAcl $observerInbox $false $false
Get-ExpectedAcl $evidenceRoot $false $false
$mutex = [Threading.Mutex]::new($false, 'Global\SynveilP028-' + $RunId)
$hasLock = $false
try {
    try { $hasLock = $mutex.WaitOne([TimeSpan]::FromSeconds(10)) } catch [Threading.AbandonedMutexException] { $hasLock = $true }
    if (-not $hasLock) { throw 'P028_COORDINATOR_LOCK_TIMEOUT' }
    [void](Assert-NoReparsePath $statePath)
    $stateBytes = [P028SafeEvidenceReader]::Read($statePath, 65536)
    if ($stateBytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $state = [Text.Encoding]::UTF8.GetString($stateBytes) | ConvertFrom-Json
    if ($state.schema_version -ne 1 -or $state.run_id -cne $RunId -or @('ACTIVE','FINALIZED') -cnotcontains $state.status -or
        $state.source_commit -cne $SourceCommit -or $state.harness_commit -cne $HarnessCommit -or
        $state.candidate_sha256 -cne $CandidateSha256 -or $state.manifest_sha256 -cne $ManifestSha256) {
        throw 'P028_RUN_STATE_IDENTITY_MISMATCH'
    }
    $subjectHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($subject.Value))).ToLowerInvariant()
    $controllerHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
    if ($state.subject_sid_sha256 -cne $subjectHash) { throw 'P028_SUBJECT_IDENTITY_MISMATCH' }
    if ($state.controller_sid_sha256 -cne $controllerHash) { throw 'P028_CONTROLLER_IDENTITY_MISMATCH' }
    if ([DateTimeOffset]::UtcNow -gt [DateTimeOffset]::Parse($state.expires_at_utc)) { throw 'P028_RUN_EXPIRED' }
    if ((Get-MachineDigest) -cne $state.machine_guid_sha256 -or [int](Get-CimInstance Win32_OperatingSystem).BuildNumber -ne [int]$state.os_build) { throw 'P028_MACHINE_IDENTITY_MISMATCH' }
    # No authenticated native observer can Advance in this checkpoint. A restored
    # or tampered file claiming recorded phases/finalization must fail even Verify.
    if ($state.mode -cne 'WINDOWS_PROTECTED_PENDING_NATIVE_AUTHORITY' -or
        $state.status -cne 'ACTIVE' -or
        -not ($state.PSObject.Properties.Name -ccontains 'phases') -or
        -not ($state.phases -is [array]) -or
        $state.phases.Count -ne 0 -or
        -not ($state.PSObject.Properties.Name -ccontains 'finalization_digest') -or
        $null -ne $state.finalization_digest) { throw 'P028_UNAUTHORIZED_STATE_ADVANCEMENT' }
    if ($Action -eq 'Verify') { Write-Output "P028_PROTECTED_RUN_VERIFIED $RunId $($state.status)"; exit 0 }
    if ($state.status -cne 'ACTIVE') { throw 'P028_RUN_TERMINAL' }
    # JSON fields and the directory ACL do not authenticate the event collectors,
    # candidate or observer provenance. Do not record a native PASS until that
    # independent authority is implemented and qualified.
    throw 'P028_NATIVE_AUTHORITY_UNAVAILABLE'
    if ($Action -ne 'Advance' -or [string]::IsNullOrWhiteSpace($Phase) -or [string]::IsNullOrWhiteSpace($EvidenceFile)) { throw 'P028_TRANSITION_ARGUMENTS_INVALID' }
    if ($state.phases.Count -ge 13 -or $Phase -cne @('PREFLIGHT','CANDIDATE_VERIFIED','ENVIRONMENT_QUALIFIED','INSTALLED','GUI_VERIFIED','IPC_VERIFIED','STARTUP_CONFIGURED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED','LIFECYCLE_VERIFIED','CLEANUP_VERIFIED','EVIDENCE_FINALIZED')[$state.phases.Count]) { throw 'P028_PHASE_ORDER_INVALID' }
    $observerPhases = @('PREFLIGHT','CANDIDATE_VERIFIED','ENVIRONMENT_QUALIFIED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED','CLEANUP_VERIFIED','EVIDENCE_FINALIZED')
    $sourceRoot = if ($observerPhases -ccontains $Phase) { $observerInbox } else { $inboxPath }
    $sourcePath = Assert-NoReparsePath $EvidenceFile
    if ([IO.Path]::GetDirectoryName($sourcePath) -cne [IO.Path]::GetFullPath($sourceRoot) -or [IO.Path]::GetFileName($sourcePath) -cne ($Phase + '.json')) { throw 'P028_EVIDENCE_SOURCE_INVALID' }
    $raw = [P028SafeEvidenceReader]::Read($sourcePath, 4194304)
    $record = [Text.Encoding]::UTF8.GetString($raw) | ConvertFrom-Json
    $expectedSubjectHash = $state.subject_sid_sha256
    $required = @('schema_version','run_id','phase','result','source_commit','harness_commit','candidate_sha256','manifest_sha256','machine_guid_sha256','subject_sid_sha256','observer_sid_sha256','observed_at_utc','session_id','logon_identity_sha256','manual_task_run_present')
    foreach ($key in $required) { if (-not ($record.PSObject.Properties.Name -ccontains $key)) { throw 'P028_EVIDENCE_FIELD_MISSING' } }
    if ($record.schema_version -ne 1 -or $record.run_id -cne $RunId -or $record.phase -cne $Phase -or $record.result -cne 'PASS' -or
        $record.source_commit -cne $SourceCommit -or $record.harness_commit -cne $HarnessCommit -or
        $record.candidate_sha256 -cne $CandidateSha256 -or $record.manifest_sha256 -cne $ManifestSha256 -or
        $record.machine_guid_sha256 -cne $state.machine_guid_sha256 -or $record.subject_sid_sha256 -cne $expectedSubjectHash) { throw 'P028_EVIDENCE_CONTEXT_MISMATCH' }
    $observedAt = [DateTimeOffset]::Parse([string]$record.observed_at_utc)
    if ($observedAt.Offset -ne [TimeSpan]::Zero -or $observedAt -lt [DateTimeOffset]::Parse($state.created_at_utc) -or
        $observedAt -gt [DateTimeOffset]::UtcNow.AddMinutes(2) -or $observedAt -gt [DateTimeOffset]::Parse($state.expires_at_utc)) { throw 'P028_EVIDENCE_TIME_INVALID' }
    if ($state.phases.Count -gt 0 -and $observedAt -lt [DateTimeOffset]::Parse($state.phases[-1].observed_at_utc)) { throw 'P028_EVIDENCE_TIME_ORDER_INVALID' }
    if ($observerPhases -ccontains $Phase) {
        $observerHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant()
        if ($record.observer_sid_sha256 -cne $observerHash) { throw 'P028_OBSERVER_IDENTITY_MISMATCH' }
    }
    if (@('GUI_VERIFIED','LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED') -ccontains $Phase) {
        if ([int]$record.session_id -le 0 -or $record.logon_identity_sha256 -notmatch '^[0-9a-f]{64}$') { throw 'P028_LOGON_BINDING_INVALID' }
    }
    if (@('LOGOFF_OBSERVED','NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED') -ccontains $Phase) {
        $guiEvidencePath = Join-Path $evidenceRoot 'GUI_VERIFIED.json'
        if (-not (Test-Path -LiteralPath $guiEvidencePath -PathType Leaf)) { throw 'P028_INITIAL_SESSION_EVIDENCE_MISSING' }
        $initialSession = [Text.Encoding]::UTF8.GetString([P028SafeEvidenceReader]::Read($guiEvidencePath, 4194304)) | ConvertFrom-Json
        if ($Phase -eq 'LOGOFF_OBSERVED' -and ([int]$record.session_id -ne [int]$initialSession.session_id -or $record.logon_identity_sha256 -cne $initialSession.logon_identity_sha256)) { throw 'P028_LOGOFF_IDENTITY_MISMATCH' }
        if ($Phase -in @('NEW_LOGON_OBSERVED','LOGON_TRIGGER_VERIFIED')) {
            $newEvidencePath = Join-Path $evidenceRoot 'NEW_LOGON_OBSERVED.json'
            if ($Phase -eq 'LOGON_TRIGGER_VERIFIED' -and -not (Test-Path -LiteralPath $newEvidencePath -PathType Leaf)) { throw 'P028_NEW_LOGON_EVIDENCE_MISSING' }
            $newSession = if ($Phase -eq 'LOGON_TRIGGER_VERIFIED') { [Text.Encoding]::UTF8.GetString([P028SafeEvidenceReader]::Read($newEvidencePath, 4194304)) | ConvertFrom-Json } else { $record }
            if ($newSession.logon_identity_sha256 -ceq $initialSession.logon_identity_sha256) { throw 'P028_LOGON_REPLAY' }
            if ($Phase -eq 'LOGON_TRIGGER_VERIFIED' -and ([int]$record.session_id -ne [int]$newSession.session_id -or $record.logon_identity_sha256 -cne $newSession.logon_identity_sha256)) { throw 'P028_TASK_SESSION_MISMATCH' }
        }
        if ($Phase -eq 'LOGON_TRIGGER_VERIFIED' -and $record.manual_task_run_present -ne $false) { throw 'P028_MANUAL_TASK_RUN_REJECTED' }
    }
    if ($Phase -eq 'EVIDENCE_FINALIZED' -and ($state.phases[-1].phase -cne 'CLEANUP_VERIFIED' -or $state.finalization_digest)) { throw 'P028_FINALIZATION_PRECONDITION' }
    $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($raw)).ToLowerInvariant()
    if (@($state.phases | Where-Object digest -ceq $digest).Count -gt 0) { throw 'P028_EVIDENCE_REPLAY' }
    $destination = Join-Path $evidenceRoot ($Phase + '.json')
    $destTemp = Join-Path $evidenceRoot ('.evidence-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $out = [IO.File]::Open($destTemp, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $out.Write($raw); $out.Flush($true) } finally { $out.Dispose() }
    if (Test-Path -LiteralPath $destination) {
        $existingDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([P028SafeEvidenceReader]::Read($destination, 4194304))).ToLowerInvariant()
        if ($existingDigest -cne $digest) { throw 'P028_EVIDENCE_REPLAY' }
        Remove-Item -LiteralPath $destTemp -Force
    } else { [IO.File]::Move($destTemp, $destination) }
    $now = [DateTimeOffset]::UtcNow.ToString('o')
    $state.phases += [ordered]@{phase=$Phase;digest=$digest;observer_sid_sha256=if($observerPhases -ccontains $Phase){$observerHash}else{$null};observed_at_utc=$observedAt.ToString('o');completed_at_utc=$now}
    if ($Phase -eq 'EVIDENCE_FINALIZED') { $state.finalization_digest=$digest; $state.status='FINALIZED' }
    Save-RunState $state
    Write-Output "P028_PHASE_RECORDED $Phase $digest"
} finally {
    if ($hasLock) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
