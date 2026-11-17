[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Initialize','Verify','Advance','ObserveInitialSession','ObserveLogoff','ObserveNewLogon','ObserveLogonTrigger','VerifyObserverSequence')][string]$Action,
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
$windows = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
$computerSystem = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
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
$observerSessionType = @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
[StructLayout(LayoutKind.Sequential)] public struct P028TokenStatistics {
    public long TokenId; public long AuthenticationId; public long ExpirationTime;
    public int TokenType; public int ImpersonationLevel; public uint DynamicCharged;
    public uint DynamicAvailable; public uint GroupCount; public uint PrivilegeCount; public long ModifiedId;
}
public static class P028ObserverNative {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool ProcessIdToSessionId(uint pid, out uint sessionId);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr buffer, int length, out int needed);
    [DllImport("wtsapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WTSQuerySessionInformation(IntPtr server, uint sessionId, int infoClass, out IntPtr buffer, out uint bytes);
    [DllImport("wtsapi32.dll")] static extern void WTSFreeMemory(IntPtr buffer);
    static byte[] Info(IntPtr token, int kind) {
        int needed; GetTokenInformation(token, kind, IntPtr.Zero, 0, out needed);
        if (needed <= 0 || needed > 65536) throw new Win32Exception();
        IntPtr buffer = Marshal.AllocHGlobal(needed);
        try { if (!GetTokenInformation(token, kind, buffer, needed, out needed)) throw new Win32Exception(); byte[] result = new byte[needed]; Marshal.Copy(buffer, result, 0, needed); return result; }
        finally { Marshal.FreeHGlobal(buffer); }
    }
    static bool HasGroup(IntPtr token, string expectedSid) {
        byte[] groups = Info(token, 2);
        IntPtr buffer = Marshal.AllocHGlobal(groups.Length);
        try {
            Marshal.Copy(groups, 0, buffer, groups.Length);
            uint count = unchecked((uint)Marshal.ReadInt32(buffer));
            if (count > 2048) throw new InvalidDataException("token group count is out of bounds");
            int offset = IntPtr.Size == 8 ? 8 : 4;
            int stride = IntPtr.Size + 8;
            if ((long)offset + ((long)count * stride) > groups.Length) throw new InvalidDataException("token group buffer is truncated");
            for (int index = 0; index < count; index++) {
                IntPtr sidPointer = Marshal.ReadIntPtr(buffer, offset + (index * stride));
                if (sidPointer == IntPtr.Zero) throw new InvalidDataException("token group SID is missing");
                if (new SecurityIdentifier(sidPointer).Value == expectedSid) return true;
            }
            return false;
        } finally { Marshal.FreeHGlobal(buffer); }
    }
    public static string[] ProcessToken(uint pid) {
        IntPtr process = OpenProcess(0x1000, false, pid); if (process == IntPtr.Zero) throw new Win32Exception();
        try {
            IntPtr token; if (!OpenProcessToken(process, 0x0008, out token)) throw new Win32Exception();
            try {
                byte[] user = Info(token, 1); IntPtr userBuffer = Marshal.AllocHGlobal(user.Length);
                try {
                    Marshal.Copy(user, 0, userBuffer, user.Length); string sid = new SecurityIdentifier(Marshal.ReadIntPtr(userBuffer)).Value;
                    byte[] statsBytes = Info(token, 10); IntPtr statsBuffer = Marshal.AllocHGlobal(statsBytes.Length);
                    try {
                        Marshal.Copy(statsBytes, 0, statsBuffer, statsBytes.Length); P028TokenStatistics stats = Marshal.PtrToStructure<P028TokenStatistics>(statsBuffer);
                        uint session; if (!ProcessIdToSessionId(pid, out session)) throw new Win32Exception();
                        byte[] elevation = Info(token, 20);
                        if (elevation.Length < 4) throw new InvalidDataException("token elevation information is malformed");
                        bool elevated = BitConverter.ToInt32(elevation, 0) != 0;
                        bool administrator = HasGroup(token, "S-1-5-32-544");
                        return new[]{sid, "0x" + unchecked((ulong)stats.AuthenticationId).ToString("x"), session.ToString(), elevated ? "true" : "false", administrator ? "true" : "false"};
                    } finally { Marshal.FreeHGlobal(statsBuffer); }
                } finally { Marshal.FreeHGlobal(userBuffer); }
            } finally { CloseHandle(token); }
        } finally { CloseHandle(process); }
    }
    public static bool WtsSessionIsActive(uint session) {
        IntPtr buffer; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 8, out buffer, out bytes)) return false;
        try { return Marshal.ReadInt32(buffer) == 0; } finally { WTSFreeMemory(buffer); }
    }
    public static string WtsSessionUser(uint session) {
        IntPtr buffer; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 5, out buffer, out bytes)) return null;
        try { return Marshal.PtrToStringUni(buffer); } finally { WTSFreeMemory(buffer); }
    }
    public static string WtsSessionDomain(uint session) {
        IntPtr buffer; uint bytes; if (!WTSQuerySessionInformation(IntPtr.Zero, session, 7, out buffer, out bytes)) return null;
        try { return Marshal.PtrToStringUni(buffer); } finally { WTSFreeMemory(buffer); }
    }
}
'@
if (-not ('P028ObserverNative' -as [type])) { Add-Type -TypeDefinition $observerSessionType }
function Save-RunState($State) {
    $temporary = Join-Path $trustedPath ('.run-state-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($State | ConvertTo-Json -Depth 12) + "`n")
    if ($bytes.Length -gt 65536) { throw 'P028_STATE_TOO_LARGE' }
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $statePath, $true)
}
function Hash-P028Text([string]$Value) {
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value))).ToLowerInvariant()
}
function Normalize-P028LogonId([string]$Value) {
    if ($Value -notmatch '^(?:0x)?[0-9a-fA-F]{1,16}$') { return $null }
    try { return [Convert]::ToUInt64(($Value -replace '^0x',''),16).ToString('x') } catch { return $null }
}
function Resolve-P028Sid([string]$IdentityName) {
    if ([string]::IsNullOrWhiteSpace($IdentityName)) { return $null }
    if ($IdentityName -match '^S-1-') { return $IdentityName }
    try { return ([Security.Principal.NTAccount]::new($IdentityName)).Translate([Security.Principal.SecurityIdentifier]).Value }
    catch { return $null }
}
function Get-P028Event([object]$Event, [int]$ExpectedId, [string]$Channel, [string[]]$RequiredData) {
    if ($null -eq $Event -or $Event.Id -ne $ExpectedId -or $Event.LogName -cne $Channel -or
        $Event.ProviderName -cne 'Microsoft-Windows-Security-Auditing' -or $null -eq $Event.RecordId -or [long]$Event.RecordId -le 0 -or
        $null -eq $Event.TimeCreated) { throw 'P028_EVENT_SOURCE_OR_ID_INVALID' }
    $eventXml = [string]$Event.ToXml()
    if ([string]::IsNullOrWhiteSpace($eventXml) -or $eventXml.Length -gt 65536) { throw 'P028_EVENT_XML_SIZE_INVALID' }
    try { [xml]$xml = $eventXml } catch { throw 'P028_EVENT_XML_INVALID' }
    $provider = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="Provider"]')
    $systemId = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="EventID"]')
    $recordId = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="EventRecordID"]')
    $channelNode = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="Channel"]')
    $computer = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="Computer"]')
    $timeNode = $xml.SelectSingleNode('//*[local-name()="System"]/*[local-name()="TimeCreated"]')
    if ($null -eq $provider -or $provider.GetAttribute('Name') -cne 'Microsoft-Windows-Security-Auditing' -or
        $null -eq $systemId -or [int]$systemId.InnerText -ne $ExpectedId -or $null -eq $recordId -or
        [long]$recordId.InnerText -ne [long]$Event.RecordId -or $null -eq $channelNode -or $channelNode.InnerText -cne $Channel -or
        $null -eq $computer -or $null -eq $timeNode) { throw 'P028_EVENT_SCHEMA_MISMATCH' }
    $knownComputers = @([string]$env:COMPUTERNAME)
    if (-not [string]::IsNullOrWhiteSpace([string]$computerSystem.Domain) -and [string]$computerSystem.Domain -notmatch '^(?:WORKGROUP|LOCAL)$') {
        $knownComputers += ([string]$env:COMPUTERNAME + '.' + [string]$computerSystem.Domain)
    }
    if (@($knownComputers | Where-Object { $_ -ieq [string]$computer.InnerText }).Count -ne 1) { throw 'P028_EVENT_MACHINE_MISMATCH' }
    try { $eventTime = [DateTimeOffset]::Parse([string]$timeNode.GetAttribute('SystemTime')).ToUniversalTime() }
    catch { throw 'P028_EVENT_TIME_INVALID' }
    if ([Math]::Abs(($eventTime - [DateTimeOffset]$Event.TimeCreated.ToUniversalTime()).TotalSeconds) -gt 2) { throw 'P028_EVENT_TIME_MISMATCH' }
    $data = @{}
    foreach ($node in $xml.SelectNodes('//*[local-name()="EventData"]/*[local-name()="Data"]')) {
        $name = [string]$node.GetAttribute('Name')
        if ([string]::IsNullOrWhiteSpace($name) -or $data.ContainsKey($name)) { throw 'P028_EVENT_DATA_AMBIGUOUS' }
        $data[$name] = [string]$node.InnerText
    }
    foreach ($name in $RequiredData) {
        if (-not $data.ContainsKey($name) -or [string]::IsNullOrWhiteSpace([string]$data[$name])) { throw 'P028_EVENT_FIELD_MISSING' }
    }
    $xmlBytes = [Text.Encoding]::UTF8.GetBytes($eventXml)
    return [pscustomobject]@{
        Id=$ExpectedId; RecordId=[long]$Event.RecordId; TimeUtc=$eventTime; Data=$data
        XmlSha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($xmlBytes)).ToLowerInvariant()
        Channel=$Channel; Provider='Microsoft-Windows-Security-Auditing'
    }
}
function Get-P028SubjectSession([string]$ExpectedSid, [bool]$RequireOne = $true) {
    $sessions = [Collections.Generic.List[object]]::new()
    $processes = @(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" -ErrorAction Stop)
    foreach ($process in $processes) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction Stop
        if ($owner.ReturnValue -ne 0 -or [string]::IsNullOrWhiteSpace([string]$owner.Sid)) { throw 'P028_SESSION_PROCESS_OWNER_UNVERIFIABLE' }
        if ([string]$owner.Sid -cne $ExpectedSid) { continue }
        try { $token = [P028ObserverNative]::ProcessToken([uint32]$process.ProcessId) }
        catch { throw 'P028_SESSION_TOKEN_UNVERIFIABLE' }
        $sessionId = [uint32]$token[2]
        if ([string]$token[0] -cne $ExpectedSid -or $sessionId -ne [uint32]$process.SessionId) { throw 'P028_SESSION_TOKEN_MISMATCH' }
        if ([string]$token[3] -cne 'false' -or [string]$token[4] -cne 'false') { throw 'P028_SUBJECT_TOKEN_NOT_ORDINARY_USER' }
        if (-not [P028ObserverNative]::WtsSessionIsActive($sessionId)) { continue }
        $user = [P028ObserverNative]::WtsSessionUser($sessionId)
        $domain = [P028ObserverNative]::WtsSessionDomain($sessionId)
        $qualified = if ([string]::IsNullOrWhiteSpace($domain)) { $user } else { "$domain\$user" }
        if ((Resolve-P028Sid $qualified) -cne $ExpectedSid) { throw 'P028_WTS_SUBJECT_MISMATCH' }
        $luid = Normalize-P028LogonId ([string]$token[1])
        if ([string]::IsNullOrWhiteSpace($luid)) { throw 'P028_SESSION_LUID_INVALID' }
        $sessions.Add([pscustomobject]@{SessionId=$sessionId; Luid=$luid; LuidSha256=(Hash-P028Text $luid); Sid=$ExpectedSid})
    }
    $unique = @($sessions | Sort-Object SessionId,Luid -Unique)
    if ($RequireOne -and $unique.Count -ne 1) { throw 'P028_SUBJECT_SESSION_MISSING_OR_AMBIGUOUS' }
    return $unique
}
function Read-P028ObserverRecord([string]$Filename, [string]$ActionName, $RunState) {
    $path = Join-Path $observerInbox $Filename
    [void](Assert-NoReparsePath $path)
    $bytes = [P028SafeEvidenceReader]::Read($path, 262144)
    try { $record = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json }
    catch { throw 'P028_PRIOR_OBSERVER_RECORD_INVALID' }
    if ($record.schema_version -ne 1 -or $record.result -cne 'OBSERVED' -or $record.mode -cne 'WINDOWS_NATIVE_OBSERVER' -or
        $record.action -cne $ActionName -or $record.run_id -cne $RunId -or
        $record.machine_guid_sha256 -cne $RunState.machine_guid_sha256 -or
        $record.boot_time_sha256 -cne $RunState.boot_time_sha256 -or
        $record.subject_sid_sha256 -cne $RunState.subject_sid_sha256 -or
        $record.controller_sid_sha256 -cne $RunState.controller_sid_sha256 -or
        $record.source_commit -cne $SourceCommit -or $record.harness_commit -cne $HarnessCommit -or
        $record.candidate_sha256 -cne $CandidateSha256 -or $record.manifest_sha256 -cne $ManifestSha256) {
        throw 'P028_PRIOR_OBSERVER_CONTEXT_MISMATCH'
    }
    return [pscustomobject]@{Record=$record; Bytes=$bytes; RecordDigest=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()}
}
function New-P028ObserverRecord($RunState, [string]$ActionName, $EventRecord, $Session, [object[]]$PriorRecords) {
    $now = [DateTimeOffset]::UtcNow
    $eventTime = [DateTimeOffset]$EventRecord.TimeUtc
    $created = [DateTimeOffset]::Parse([string]$RunState.created_at_utc).ToUniversalTime()
    $expires = [DateTimeOffset]::Parse([string]$RunState.expires_at_utc).ToUniversalTime()
    if ($eventTime -lt $created -or $eventTime -gt $now -or $now -gt $expires -or $now - $eventTime -gt [TimeSpan]::FromMinutes(15)) {
        throw 'P028_OBSERVER_EVENT_OUT_OF_WINDOW'
    }
    $eventKey = "$($EventRecord.Channel)|$($EventRecord.Provider)|$($EventRecord.RecordId)"
    foreach ($prior in $PriorRecords) {
        if ("$($prior.Record.event.channel)|$($prior.Record.event.provider)|$($prior.Record.event.record_id)" -ceq $eventKey) {
            throw 'P028_OBSERVER_EVENT_REPLAY'
        }
    }
    return [ordered]@{
        schema_version=1; mode='WINDOWS_NATIVE_OBSERVER'; result='OBSERVED'; action=$ActionName
        run_id=$RunId; source_commit=$SourceCommit; harness_commit=$HarnessCommit
        candidate_sha256=$CandidateSha256; manifest_sha256=$ManifestSha256
        machine_guid_sha256=$RunState.machine_guid_sha256; boot_time_sha256=$RunState.boot_time_sha256; subject_sid_sha256=$RunState.subject_sid_sha256
        observer_sid_sha256=(Hash-P028Text $identity.User.Value); controller_sid_sha256=$RunState.controller_sid_sha256
        event=[ordered]@{channel=$EventRecord.Channel;provider=$EventRecord.Provider;event_id=$EventRecord.Id
            record_id=$EventRecord.RecordId;occurred_at_utc=$eventTime.ToString('yyyy-MM-ddTHH:mm:ssZ')
            record_sha256=$EventRecord.XmlSha256;machine_guid_sha256=$RunState.machine_guid_sha256;boot_time_sha256=$RunState.boot_time_sha256
            logon_type=if($EventRecord.Data.ContainsKey('LogonType')){[int]$EventRecord.Data.LogonType}else{$null}}
        session=[ordered]@{windows_session_id=[uint32]$Session.SessionId;authentication_luid_sha256=$Session.LuidSha256
            user_sid_sha256=$RunState.subject_sid_sha256}
        prior_record_sha256=@($PriorRecords | ForEach-Object { $_.RecordDigest })
        observed_at_utc=$now.ToString('yyyy-MM-ddTHH:mm:ssZ')
        redaction=[ordered]@{raw_event_xml_recorded=$false;raw_logon_id_recorded=$false;user_names_recorded=$false;machine_guid_recorded=$false}
    }
}
function Save-P028ObserverRecord($Record, [string]$Filename) {
    $destination = Join-Path $observerInbox $Filename
    [void](Assert-NoReparsePath $destination)
    if (Test-Path -LiteralPath $destination) { throw 'P028_OBSERVER_RECORD_ALREADY_EXISTS' }
    $temporary = Join-Path $observerInbox ('.observer-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Record | ConvertTo-Json -Depth 8 -Compress) + "`n")
    if ($bytes.Length -gt 262144) { throw 'P028_OBSERVER_RECORD_TOO_LARGE' }
    try {
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
        [IO.File]::Move($temporary, $destination)
    } catch {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
        throw 'P028_OBSERVER_RECORD_COMMIT_FAILED'
    }
    [void](Assert-NoReparsePath $destination)
    [void][P028SafeEvidenceReader]::Read($destination, 262144)
}
function Get-P028SecurityEvents([int]$EventId, [DateTime]$StartTime) {
    try {
        $events = @(Get-WinEvent -FilterHashtable @{LogName='Security';Id=$EventId;StartTime=$StartTime;EndTime=[DateTime]::UtcNow} -MaxEvents 4097 -ErrorAction Stop)
    } catch { throw 'P028_OBSERVER_SECURITY_CHANNEL_UNAVAILABLE' }
    if ($events.Count -gt 4096) { throw 'P028_OBSERVER_EVENT_QUERY_TRUNCATED' }
    return ,$events
}
function Get-P028SecurityEventByRecordId([int]$EventId, [long]$RecordId) {
    if ($EventId -notin @(4624,4634) -or $RecordId -le 0) { throw 'P028_EVENT_RECORD_REFERENCE_INVALID' }
    # EventRecordID is numeric and bounded above, so it is safe to interpolate
    # into this XPath. Re-read the OS event log instead of trusting persisted
    # observer JSON or its digest as an authority.
    $filter = "*[System[EventRecordID=$RecordId]]"
    try { $events = @(Get-WinEvent -LogName 'Security' -FilterXPath $filter -MaxEvents 2 -ErrorAction Stop) }
    catch { throw 'P028_OBSERVER_EVENT_REQUERY_FAILED' }
    if ($events.Count -ne 1) { throw 'P028_OBSERVER_EVENT_REQUERY_AMBIGUOUS' }
    $required = if ($EventId -eq 4624) { @('TargetUserSid','TargetLogonId','LogonType') } else { @('TargetUserSid','TargetLogonId') }
    return Get-P028Event $events[0] $EventId 'Security' $required
}
function Assert-P028ObserverEventRequery($Record, [int]$ExpectedId, [string]$ExpectedLuidHash, [string]$ExpectedSid) {
    if ([int]$Record.event.event_id -ne $ExpectedId -or
        [long]$Record.event.record_id -le 0 -or
        [string]$Record.event.record_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'P028_OBSERVER_EVENT_REFERENCE_INVALID'
    }
    $actual = Get-P028SecurityEventByRecordId $ExpectedId ([long]$Record.event.record_id)
    $storedTime = [DateTimeOffset]::Parse([string]$Record.event.occurred_at_utc).ToUniversalTime()
    $actualLuid = Normalize-P028LogonId ([string]$actual.Data.TargetLogonId)
    if ([string]::IsNullOrWhiteSpace($actualLuid)) { throw 'P028_OBSERVER_EVENT_LOGON_ID_INVALID' }
    if ($actual.XmlSha256 -cne [string]$Record.event.record_sha256 -or
        [Math]::Abs(($actual.TimeUtc - $storedTime).TotalSeconds) -ge 1 -or
        [string]$actual.Data.TargetUserSid -cne $ExpectedSid -or
        (Hash-P028Text $actualLuid) -cne $ExpectedLuidHash) {
        throw 'P028_OBSERVER_EVENT_REQUERY_MISMATCH'
    }
    if ($ExpectedId -eq 4624 -and [string]$actual.Data.LogonType -notin @('2','10')) {
        throw 'P028_OBSERVER_EVENT_LOGON_TYPE_MISMATCH'
    }
}
function Invoke-P028ObserverAction($RunState, [string]$RequestedAction) {
    $subjectSid = $subject.Value
    $initialPath = 'P028-INITIAL-SESSION-v1.json'
    $logoffPath = 'P028-LOGOFF-v1.json'
    $prior = [Collections.Generic.List[object]]::new()
    $initial = $null
    $logoff = $null
    if ($RequestedAction -in @('ObserveLogoff','ObserveNewLogon')) {
        $initial = Read-P028ObserverRecord $initialPath 'ObserveInitialSession' $RunState
        $prior.Add([pscustomobject]@{Record=$initial.Record;RecordDigest=$initial.RecordDigest})
    }
    if ($RequestedAction -eq 'ObserveNewLogon') {
        $logoff = Read-P028ObserverRecord $logoffPath 'ObserveLogoff' $RunState
        $prior.Add([pscustomobject]@{Record=$logoff.Record;RecordDigest=$logoff.RecordDigest})
    }
    if ($RequestedAction -eq 'ObserveInitialSession') {
        $active = @(Get-P028SubjectSession $subjectSid)
        $session = $active[0]
        $events = Get-P028SecurityEvents 4624 ([DateTime]([DateTimeOffset]::Parse([string]$RunState.created_at_utc).UtcDateTime))
        $matches = [Collections.Generic.List[object]]::new()
        foreach ($item in $events) {
            $parsed = Get-P028Event $item 4624 'Security' @('TargetUserSid','TargetLogonId','LogonType')
            $eventSid = [string]$parsed.Data.TargetUserSid
            $eventLuid = Normalize-P028LogonId ([string]$parsed.Data.TargetLogonId)
            if ($eventSid -ceq $subjectSid -and $eventLuid -and (Hash-P028Text $eventLuid) -ceq $session.LuidSha256 -and
                [string]$parsed.Data.LogonType -in @('2','10')) { $matches.Add($parsed) }
        }
        if ($matches.Count -ne 1) { throw 'P028_INITIAL_SESSION_EVENT_MISSING_OR_AMBIGUOUS' }
        $record = New-P028ObserverRecord $RunState $RequestedAction $matches[0] $session @()
        Save-P028ObserverRecord $record $initialPath
        Write-Output 'P028_OBSERVER_RECORD_RECORDED ObserveInitialSession'
        return
    }
    if ($RequestedAction -eq 'ObserveLogoff') {
        $initialRecord = $initial.Record
        $initialEventTime = [DateTimeOffset]::Parse([string]$initialRecord.event.occurred_at_utc).UtcDateTime
        $initialSession = [pscustomobject]@{SessionId=[uint32]$initialRecord.session.windows_session_id
            LuidSha256=[string]$initialRecord.session.authentication_luid_sha256}
        $events = Get-P028SecurityEvents 4634 $initialEventTime
        $matches = [Collections.Generic.List[object]]::new()
        foreach ($item in $events) {
            $parsed = Get-P028Event $item 4634 'Security' @('TargetUserSid','TargetLogonId')
            $eventSid = [string]$parsed.Data.TargetUserSid
            $eventLuid = Normalize-P028LogonId ([string]$parsed.Data.TargetLogonId)
            if ($eventSid -ceq $subjectSid -and $eventLuid -and (Hash-P028Text $eventLuid) -ceq $initialSession.LuidSha256 -and
                $parsed.TimeUtc -gt $initialEventTime) { $matches.Add($parsed) }
        }
        if ($matches.Count -ne 1) { throw 'P028_LOGOFF_EVENT_MISSING_OR_AMBIGUOUS' }
        if ([P028ObserverNative]::WtsSessionIsActive($initialSession.SessionId)) { throw 'P028_INITIAL_WINDOWS_SESSION_STILL_ACTIVE' }
        $record = New-P028ObserverRecord $RunState $RequestedAction $matches[0] $initialSession @($initial)
        Save-P028ObserverRecord $record $logoffPath
        Write-Output 'P028_OBSERVER_RECORD_RECORDED ObserveLogoff'
        return
    }
    if ($RequestedAction -eq 'ObserveNewLogon') {
        $initialRecord = $initial.Record
        $logoffRecord = $logoff.Record
        $initialLuidHash = [string]$initialRecord.session.authentication_luid_sha256
        $logoffTime = [DateTimeOffset]::Parse([string]$logoffRecord.event.occurred_at_utc).UtcDateTime
        $active = @(Get-P028SubjectSession $subjectSid)
        $session = $active[0]
        if ($session.LuidSha256 -ceq $initialLuidHash) { throw 'P028_AUTHENTICATION_SESSION_REUSED' }
        $events = Get-P028SecurityEvents 4624 $logoffTime
        $matches = [Collections.Generic.List[object]]::new()
        foreach ($item in $events) {
            $parsed = Get-P028Event $item 4624 'Security' @('TargetUserSid','TargetLogonId','LogonType')
            $eventSid = [string]$parsed.Data.TargetUserSid
            $eventLuid = Normalize-P028LogonId ([string]$parsed.Data.TargetLogonId)
            if ($eventSid -ceq $subjectSid -and $eventLuid -and (Hash-P028Text $eventLuid) -ceq $session.LuidSha256 -and
                [string]$parsed.Data.LogonType -in @('2','10') -and $parsed.TimeUtc -gt $logoffTime) { $matches.Add($parsed) }
        }
        if ($matches.Count -ne 1) { throw 'P028_NEW_LOGON_EVENT_MISSING_OR_AMBIGUOUS' }
        $record = New-P028ObserverRecord $RunState $RequestedAction $matches[0] $session @($initial,$logoff)
        Save-P028ObserverRecord $record 'P028-NEW-LOGON-v1.json'
        Write-Output 'P028_OBSERVER_RECORD_RECORDED ObserveNewLogon'
        return
    }
    throw 'P028_OBSERVER_ACTION_UNSUPPORTED'
}
function Verify-P028ObserverSequence($RunState) {
    $initial = Read-P028ObserverRecord 'P028-INITIAL-SESSION-v1.json' 'ObserveInitialSession' $RunState
    $logoff = Read-P028ObserverRecord 'P028-LOGOFF-v1.json' 'ObserveLogoff' $RunState
    $newLogon = Read-P028ObserverRecord 'P028-NEW-LOGON-v1.json' 'ObserveNewLogon' $RunState
    $subjectSid = $subject.Value
    Assert-P028ObserverEventRequery $initial.Record 4624 ([string]$initial.Record.session.authentication_luid_sha256) $subjectSid
    Assert-P028ObserverEventRequery $logoff.Record 4634 ([string]$initial.Record.session.authentication_luid_sha256) $subjectSid
    Assert-P028ObserverEventRequery $newLogon.Record 4624 ([string]$newLogon.Record.session.authentication_luid_sha256) $subjectSid
    $records = @($initial.Record,$logoff.Record,$newLogon.Record)
    $expectedObserver = Hash-P028Text $identity.User.Value
    foreach ($record in $records) {
        if ($record.observer_sid_sha256 -cne $expectedObserver -or
            $record.event.channel -cne 'Security' -or $record.event.provider -cne 'Microsoft-Windows-Security-Auditing' -or
            [long]$record.event.record_id -le 0 -or $record.event.record_sha256 -notmatch '^[0-9a-f]{64}$' -or
            [string]$record.event.machine_guid_sha256 -cne $RunState.machine_guid_sha256 -or
            [string]$record.boot_time_sha256 -cne $RunState.boot_time_sha256 -or
            [string]$record.session.user_sid_sha256 -cne $RunState.subject_sid_sha256 -or
            [int]$record.session.windows_session_id -le 0 -or
            [string]$record.session.authentication_luid_sha256 -notmatch '^[0-9a-f]{64}$') {
            throw 'P028_OBSERVER_SEQUENCE_BINDING_INVALID'
        }
        $eventTime = [DateTimeOffset]::Parse([string]$record.event.occurred_at_utc).ToUniversalTime()
        $observedTime = [DateTimeOffset]::Parse([string]$record.observed_at_utc).ToUniversalTime()
        if ($eventTime -gt $observedTime -or $observedTime -gt [DateTimeOffset]::UtcNow -or
            [DateTimeOffset]::UtcNow - $observedTime -gt [TimeSpan]::FromMinutes(15)) {
            throw 'P028_OBSERVER_SEQUENCE_TIME_INVALID'
        }
    }
    if ([int]$initial.Record.event.event_id -ne 4624 -or [int]$logoff.Record.event.event_id -ne 4634 -or
        [int]$newLogon.Record.event.event_id -ne 4624 -or
        [string]$logoff.Record.session.authentication_luid_sha256 -cne [string]$initial.Record.session.authentication_luid_sha256 -or
        [int]$logoff.Record.session.windows_session_id -ne [int]$initial.Record.session.windows_session_id -or
        [string]$newLogon.Record.session.authentication_luid_sha256 -ceq [string]$initial.Record.session.authentication_luid_sha256) {
        throw 'P028_OBSERVER_SEQUENCE_SESSION_MISMATCH'
    }
    $initialTime = [DateTimeOffset]::Parse([string]$initial.Record.event.occurred_at_utc).ToUniversalTime()
    $logoffTime = [DateTimeOffset]::Parse([string]$logoff.Record.event.occurred_at_utc).ToUniversalTime()
    $newLogonTime = [DateTimeOffset]::Parse([string]$newLogon.Record.event.occurred_at_utc).ToUniversalTime()
    if ($initialTime -ge $logoffTime -or $logoffTime -ge $newLogonTime) { throw 'P028_OBSERVER_SEQUENCE_ORDER_INVALID' }
    $newReferences = @($newLogon.Record.prior_record_sha256)
    if ($newReferences.Count -ne 2 -or $newReferences[0] -cne $initial.RecordDigest -or $newReferences[1] -cne $logoff.RecordDigest) {
        throw 'P028_OBSERVER_SEQUENCE_REPLAY_OR_CHAIN_INVALID'
    }
    $eventKeys = @($records | ForEach-Object { "$($_.event.channel)|$($_.event.provider)|$($_.event.record_id)" })
    if (@($eventKeys | Select-Object -Unique).Count -ne $eventKeys.Count) { throw 'P028_OBSERVER_SEQUENCE_EVENT_REPLAY' }
    # The current Task Scheduler event evidence does not bind a task instance to
    # the new authentication LUID. Keep the sequence blocked at this boundary.
    Write-Output 'P028_BLOCKED_OBSERVER_SEQUENCE_LOGON_TRIGGER_UNSUPPORTED'
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
        machine_guid_sha256=(Get-MachineDigest); boot_time_sha256=(Hash-P028Text ($windows.LastBootUpTime.ToUniversalTime().ToString('o')))
        os_build=[int]$windows.BuildNumber; architecture='x86_64'; created_at_utc=[DateTimeOffset]::UtcNow.ToString('o')
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
    $currentWindows = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    if ((Get-MachineDigest) -cne $state.machine_guid_sha256 -or [int]$currentWindows.BuildNumber -ne [int]$state.os_build) { throw 'P028_MACHINE_IDENTITY_MISMATCH' }
    if ($state.boot_time_sha256 -notmatch '^[0-9a-f]{64}$' -or
        (Hash-P028Text ($currentWindows.LastBootUpTime.ToUniversalTime().ToString('o'))) -cne $state.boot_time_sha256) { throw 'P028_MACHINE_BOOT_IDENTITY_MISMATCH' }
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
    # These read-only observer actions query local OS facilities directly and
    # write OBSERVED records only. They cannot advance coordinator phases.
    if ($Action -eq 'ObserveLogonTrigger') {
        Write-Output 'P028_BLOCKED_LOGON_TRIGGER_PROVENANCE_UNAVAILABLE'
        exit 2
    }
    if ($Action -eq 'VerifyObserverSequence') {
        Verify-P028ObserverSequence $state
        exit 2
    }
    if ($Action -in @('ObserveInitialSession','ObserveLogoff','ObserveNewLogon')) {
        Invoke-P028ObserverAction $state $Action
        exit 0
    }
    # JSON fields and the directory ACL do not yet authenticate native phase
    # advancement. Do not record a native PASS until the whole authority path is qualified.
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
