# Private functions for the protected P028 observer. Dot-source only from the pinned harness checkout.
$observerSessionType = @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using Microsoft.Win32.SafeHandles;

public static class P028ObserverNative {
    [StructLayout(LayoutKind.Sequential)] struct TokenStatistics {
        public long TokenId, AuthenticationId, ExpirationTime;
        public int TokenType, ImpersonationLevel;
        public uint DynamicCharged, DynamicAvailable, GroupCount, PrivilegeCount;
        public long ModifiedId;
    }
    public sealed class ProcessIdentity {
        public string Sid, Luid, ImagePath;
        public bool Elevated, Administrator;
        public uint SessionId;
        public DateTimeOffset CreatedUtc;
    }
    [DllImport("kernel32.dll", SetLastError=true)] internal static extern SafeWaitHandle OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("advapi32.dll", SetLastError=true)] internal static extern bool OpenProcessToken(SafeWaitHandle process, uint access, out SafeAccessTokenHandle token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(SafeAccessTokenHandle token, int kind, IntPtr buffer, int length, out int needed);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageName(SafeWaitHandle process, uint flags, StringBuilder path, ref int size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(SafeWaitHandle process, out long created, out long exited, out long kernel, out long user);
    [DllImport("wtsapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool WTSQuerySessionInformation(IntPtr server, uint sessionId, int infoClass, out IntPtr buffer, out uint bytes);
    [DllImport("wtsapi32.dll")] static extern void WTSFreeMemory(IntPtr buffer);

    internal static T TokenInfo<T>(SafeAccessTokenHandle token, int kind, Func<IntPtr,int,T> read) {
        int length;
        GetTokenInformation(token, kind, IntPtr.Zero, 0, out length);
        if (length <= 0 || length > 65536) throw new Win32Exception();
        IntPtr buffer = Marshal.AllocHGlobal(length);
        try {
            int returned;
            if (!GetTokenInformation(token, kind, buffer, length, out returned)) throw new Win32Exception();
            if (returned <= 0 || returned > length) throw new InvalidDataException();
            return read(buffer, returned);
        } finally { Marshal.FreeHGlobal(buffer); }
    }
    internal static int TokenInt(SafeAccessTokenHandle token, int kind) {
        return TokenInfo(token, kind, (buffer, size) => size == 4 ? Marshal.ReadInt32(buffer) : throw new InvalidDataException());
    }
    internal static string[] GroupSids(SafeAccessTokenHandle token) {
        return TokenInfo(token, 2, (buffer, size) => {
            int offset = IntPtr.Size;
            int stride = 2 * IntPtr.Size;
            if (size < offset) throw new InvalidDataException();
            int count = Marshal.ReadInt32(buffer);
            if (count < 0 || offset + (long)count * stride > size) throw new InvalidDataException();
            var groups = new string[count];
            for (int index = 0; index < count; index++) {
                IntPtr sid = Marshal.ReadIntPtr(buffer, offset + index * stride);
                long relative = sid.ToInt64() - buffer.ToInt64();
                if (relative < 0 || relative > size - 8 ||
                    relative + 8 + 4 * Marshal.ReadByte(sid, 1) > size) throw new InvalidDataException();
                groups[index] = new SecurityIdentifier(sid).Value;
            }
            return groups;
        });
    }
    public static ProcessIdentity ProcessToken(uint pid, string ordinarySid) {
        using (var process = OpenProcess(0x1000, false, pid)) {
            if (process.IsInvalid) throw new Win32Exception();
            SafeAccessTokenHandle token;
            if (!OpenProcessToken(process, 0x0008, out token)) throw new Win32Exception();
            using (token)
            using (var identity = new WindowsIdentity(token.DangerousGetHandle())) {
                if (identity.User.Value != ordinarySid) throw new InvalidDataException("P028_ROLE_TOKEN_SID_MISMATCH");
                P028ObserverAccess.AssertOrdinaryToken(token, identity, false);
                long created, exited, kernel, user;
                if (!GetProcessTimes(process, out created, out exited, out kernel, out user) || exited != 0) throw new InvalidDataException();
                var path = new StringBuilder(32768);
                int size = path.Capacity;
                if (!QueryFullProcessImageName(process, 0, path, ref size)) throw new Win32Exception();
                var stats = TokenInfo(token, 10, (buffer, length) => length == Marshal.SizeOf<TokenStatistics>()
                    ? Marshal.PtrToStructure<TokenStatistics>(buffer) : throw new InvalidDataException());
                return new ProcessIdentity {
                    Sid = identity.User.Value,
                    Luid = "0x" + unchecked((ulong)stats.AuthenticationId).ToString("x"),
                    SessionId = checked((uint)TokenInt(token, 12)),
                    Elevated = TokenInt(token, 20) != 0,
                    Administrator = Array.IndexOf(GroupSids(token), "S-1-5-32-544") >= 0,
                    CreatedUtc = new DateTimeOffset(DateTime.FromFileTimeUtc(created)),
                    ImagePath = path.ToString()
                };
            }
        }
    }
    public static bool WtsSessionIsActive(uint session) {
        IntPtr buffer; uint bytes;
        if (!WTSQuerySessionInformation(IntPtr.Zero, session, 8, out buffer, out bytes)) {
            int error = Marshal.GetLastWin32Error();
            if (error == 7022) return false; // ERROR_CTX_WINSTATION_NOT_FOUND
            throw new Win32Exception(error);
        }
        try {
            if (bytes != 4 || buffer == IntPtr.Zero) throw new InvalidDataException();
            return Marshal.ReadInt32(buffer) == 0;
        } finally { WTSFreeMemory(buffer); }
    }
    static string WtsName(uint session, int kind) {
        IntPtr buffer; uint bytes;
        if (!WTSQuerySessionInformation(IntPtr.Zero, session, kind, out buffer, out bytes)) throw new Win32Exception();
        try {
            if (buffer == IntPtr.Zero || bytes < 2 || bytes > 2048 || bytes % 2 != 0 ||
                Marshal.ReadInt16(buffer, checked((int)bytes - 2)) != 0) throw new InvalidDataException();
            return Marshal.PtrToStringUni(buffer, checked((int)bytes / 2 - 1));
        } finally { WTSFreeMemory(buffer); }
    }
    public static string WtsSessionUser(uint session) { return WtsName(session, 5); }
    public static string WtsSessionDomain(uint session) { return WtsName(session, 7); }
}
public static class P028ObserverAccess {
    [StructLayout(LayoutKind.Sequential)] struct GenericMapping { public uint Read, Write, Execute, All; }
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool DuplicateToken(SafeAccessTokenHandle token, int level, out SafeAccessTokenHandle duplicate);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool AccessCheck(byte[] descriptor, SafeAccessTokenHandle token,
        uint desired, ref GenericMapping mapping, byte[] privileges, ref uint length, out uint granted, out bool allowed);
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern bool LookupPrivilegeValue(string system, string name, out long luid);

    static T WithToken<T>(uint pid, uint access, string expectedSid, Func<SafeAccessTokenHandle,WindowsIdentity,T> inspect) {
        using (var process = P028ObserverNative.OpenProcess(0x1000, false, pid)) {
            if (process.IsInvalid) throw new Win32Exception();
            SafeAccessTokenHandle token;
            if (!P028ObserverNative.OpenProcessToken(process, access, out token)) throw new Win32Exception();
            using (token)
            using (var identity = new WindowsIdentity(token.DangerousGetHandle())) {
                if (identity.User.Value != expectedSid) throw new InvalidDataException("P028_ROLE_TOKEN_SID_MISMATCH");
                return inspect(token, identity);
            }
        }
    }
    public static bool IsElevated(uint pid, string expectedSid) {
        return WithToken(pid, 8, expectedSid, (token, identity) => P028ObserverNative.TokenInt(token, 20) == 1);
    }
    public static void AssertRoleToken(uint pid, string expectedSid, bool observer) {
        WithToken(pid, 8, expectedSid, (token, identity) => {
            AssertOrdinaryToken(token, identity, observer);
            return true;
        });
    }
    public static void AssertControllerToken(uint pid, string expectedSid) {
        WithToken(pid, 8, expectedSid, (token, identity) => {
            if (P028ObserverNative.TokenInt(token, 20) != 1 ||
                P028ObserverNative.TokenInt(token, 21) != 0 || P028ObserverNative.TokenInt(token, 29) != 0 ||
                !new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator))
                throw new InvalidDataException("P028_CONTROLLER_TOKEN_UNVERIFIABLE");
            var allowed = new System.Collections.Generic.HashSet<string>(StringComparer.Ordinal) {
                "S-1-1-0", "S-1-2-0", "S-1-2-1", "S-1-5-4", "S-1-5-11", "S-1-5-15",
                "S-1-5-32-544", "S-1-5-32-545", "S-1-5-32-555", "S-1-5-113", "S-1-5-114",
                "S-1-16-8192", "S-1-16-12288"
            };
            foreach (string sid in P028ObserverNative.GroupSids(token)) {
                if (allowed.Contains(sid) || System.Text.RegularExpressions.Regex.IsMatch(
                    sid, @"^S-1-5-5-[0-9]+-[0-9]+$", System.Text.RegularExpressions.RegexOptions.CultureInvariant))
                    continue;
                throw new InvalidDataException("P028_CONTROLLER_UNEXPECTED_GROUP");
            }
            return true;
        });
    }
    internal static void AssertOrdinaryToken(SafeAccessTokenHandle token, WindowsIdentity identity, bool observer) {
        if (P028ObserverNative.TokenInt(token, 20) != 0) throw new InvalidDataException("P028_ROLE_TOKEN_ELEVATED");
        if (P028ObserverNative.TokenInt(token, 21) != 0 || P028ObserverNative.TokenInt(token, 29) != 0)
            throw new InvalidDataException("P028_ROLE_TOKEN_RESTRICTED");
        var readers = new SecurityIdentifier("S-1-5-32-573");
        var knownGroups = new System.Collections.Generic.HashSet<string>(StringComparer.Ordinal) {
            "S-1-1-0", "S-1-2-0", "S-1-2-1", "S-1-5-4", "S-1-5-11", "S-1-5-15",
            "S-1-5-32-545", "S-1-5-32-555", "S-1-5-113", "S-1-16-8192"
        };
        if (observer) knownGroups.Add(readers.Value);
        foreach (string sid in P028ObserverNative.GroupSids(token)) {
            if (knownGroups.Contains(sid) || System.Text.RegularExpressions.Regex.IsMatch(
                sid, @"^S-1-5-5-[0-9]+-[0-9]+$", System.Text.RegularExpressions.RegexOptions.CultureInvariant))
                continue;
            if (sid.StartsWith("S-1-5-32-", StringComparison.Ordinal) ||
                sid.StartsWith("S-1-5-21-", StringComparison.Ordinal))
                throw new InvalidDataException("P028_ROLE_UNEXPECTED_NESTED_GROUP");
            throw new InvalidDataException("P028_ROLE_UNSUPPORTED_GROUP");
        }
        if (observer && !new WindowsPrincipal(identity).IsInRole(readers))
            throw new InvalidDataException("P028_OBSERVER_EVENT_READERS_REQUIRED");
        string[] ordinaryPrivileges = { "SeChangeNotifyPrivilege", "SeShutdownPrivilege",
            "SeUndockPrivilege", "SeIncreaseWorkingSetPrivilege", "SeTimeZonePrivilege" };
        long[] allowed = Array.ConvertAll(ordinaryPrivileges, name => {
            long value;
            if (!LookupPrivilegeValue(null, name, out value)) throw new Win32Exception();
            return value;
        });
        P028ObserverNative.TokenInfo(token, 3, (buffer, size) => {
            if (size < 4) throw new InvalidDataException();
            int count = Marshal.ReadInt32(buffer);
            // TOKEN_PRIVILEGES packs LUID_AND_ATTRIBUTES as 12 bytes.
            if (count < 0 || 4L + 12L * count > size) throw new InvalidDataException();
            for (int index = 0; index < count; index++)
                if (Array.IndexOf(allowed, Marshal.ReadInt64(buffer, 4 + 12 * index)) < 0)
                    throw new InvalidDataException("P028_ROLE_UNEXPECTED_PRIVILEGE");
            return true;
        });
    }
    public static uint MaximumAccess(uint pid, string expectedSid, string role, byte[] descriptor) {
        return WithToken(pid, 0x000A, expectedSid, (token, identity) => {
            if (role == "Controller") {
                AssertControllerToken(pid, expectedSid);
            } else {
                if (role != "Observer" && role != "Subject") throw new InvalidDataException("P028_ROLE_TOKEN_INVALID");
                AssertOrdinaryToken(token, identity, role == "Observer");
            }
            SafeAccessTokenHandle duplicate;
            if (!DuplicateToken(token, 2, out duplicate)) throw new Win32Exception();
            using (duplicate) {
                // FILE_GENERIC_{READ,WRITE,EXECUTE}, FILE_ALL_ACCESS.
                var mapping = new GenericMapping { Read=0x120089, Write=0x120116, Execute=0x1200A0, All=0x1F01FF };
                byte[] privileges = new byte[4096];
                uint length = (uint)privileges.Length, granted;
                bool allowed;
                if (!AccessCheck(descriptor, duplicate, 0x02000000, ref mapping, privileges, ref length, out granted, out allowed))
                    throw new Win32Exception();
                return allowed ? granted : 0;
            }
        });
    }
}
'@
if (-not ('P028ObserverNative' -as [type])) { Add-Type -TypeDefinition $observerSessionType }

$safeReadType = @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.Win32.SafeHandles;

public static class P028SafeEvidenceReader {
    [StructLayout(LayoutKind.Sequential)] struct FileAttributeTagInfo { public uint Attributes, ReparseTag; }
    [StructLayout(LayoutKind.Sequential)] struct FileStandardInfo {
        public long AllocationSize, EndOfFile;
        public uint LinkCount;
        public byte DeletePending, Directory;
    }
    [StructLayout(LayoutKind.Sequential)] struct FileIdInfo {
        public ulong VolumeSerialNumber;
        public ulong FileIdLow, FileIdHigh;
    }
    public sealed class FileInspection {
        public string Sha256, FileIdentitySha256;
        public long Length;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int infoClass, out FileAttributeTagInfo info, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int infoClass, out FileStandardInfo info, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int infoClass, out FileIdInfo info, uint size);
    static SafeFileHandle Open(string path, bool directory) {
        // FILE_FLAG_OPEN_REPARSE_POINT, and BACKUP_SEMANTICS for directories.
        // FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES makes directory opens participate in
        // share checks. An attributes-only open cannot pin the name against delete/rename.
        var handle = CreateFile(path, directory ? 0x81u : 0x80000000u, 1, IntPtr.Zero, 3,
            directory ? 0x02200000u : 0x00200000u, IntPtr.Zero);
        try {
            if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            FileAttributeTagInfo info;
            if (!GetFileInformationByHandleEx(handle, 9, out info, (uint)Marshal.SizeOf<FileAttributeTagInfo>()))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            if ((info.Attributes & 0x400) != 0) throw new InvalidDataException("P028_REPARSE_POINT_REJECTED");
            if (((info.Attributes & 0x10) != 0) != directory) throw new InvalidDataException("P028_PATH_TYPE_INVALID");
            return handle;
        } catch {
            handle.Dispose();
            throw;
        }
    }
    static T ReadPinned<T>(string path, int maximum, Func<FileStream,int,SafeFileHandle,T> read) {
        var parents = new List<DirectoryInfo>();
        for (var parent = Directory.GetParent(path); parent != null; parent = parent.Parent) {
            if (parents.Count >= 64) throw new InvalidDataException("P028_PATH_DEPTH_INVALID");
            parents.Add(parent);
        }
        var handles = new List<SafeFileHandle>();
        try {
            for (int index = parents.Count - 1; index >= 0; index--) handles.Add(Open(parents[index].FullName, true));
            using (var handle = Open(path, false)) {
                FileStandardInfo info;
                if (!GetFileInformationByHandleEx(handle, 1, out info, (uint)Marshal.SizeOf<FileStandardInfo>()))
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                if (info.LinkCount != 1 || info.DeletePending != 0) throw new InvalidDataException("P028_UNSAFE_FILE_LINK");
                if (info.EndOfFile <= 0 || info.EndOfFile > maximum) throw new InvalidDataException("P028_EVIDENCE_SIZE_INVALID");
                using (var stream = new FileStream(handle, FileAccess.Read)) {
                    return read(stream, (int)info.EndOfFile, handle);
                }
            }
        } finally {
            for (int index = handles.Count - 1; index >= 0; index--) handles[index].Dispose();
        }
    }
    public static byte[] Read(string path, int maximum) {
        return ReadPinned(path, maximum, (stream, length, handle) => {
            byte[] bytes = new byte[length];
            int offset = 0;
            while (offset < bytes.Length) {
                int count = stream.Read(bytes, offset, bytes.Length - offset);
                if (count == 0) throw new EndOfStreamException("P028_EVIDENCE_CHANGED_DURING_READ");
                offset += count;
            }
            if (stream.ReadByte() != -1) throw new IOException("P028_EVIDENCE_CHANGED_DURING_READ");
            return bytes;
        });
    }
    public static string Digest(string path, int maximum) {
        return ReadPinned(path, maximum, (stream, length, handle) => {
            string result = Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
            if (stream.Position != length) throw new IOException("P028_EVIDENCE_CHANGED_DURING_READ");
            return result;
        });
    }
    public static FileInspection Inspect(string path, int maximum) {
        return ReadPinned(path, maximum, (stream, length, handle) => {
            FileIdInfo id;
            if (!GetFileInformationByHandleEx(handle, 18, out id, (uint)Marshal.SizeOf<FileIdInfo>()))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            string digest = Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
            if (stream.Position != length) throw new IOException("P028_EVIDENCE_CHANGED_DURING_READ");
            byte[] identity = new byte[24];
            Buffer.BlockCopy(BitConverter.GetBytes(id.VolumeSerialNumber), 0, identity, 0, 8);
            Buffer.BlockCopy(BitConverter.GetBytes(id.FileIdLow), 0, identity, 8, 8);
            Buffer.BlockCopy(BitConverter.GetBytes(id.FileIdHigh), 0, identity, 16, 8);
            return new FileInspection {
                Sha256 = digest,
                FileIdentitySha256 = Convert.ToHexString(SHA256.HashData(identity)).ToLowerInvariant(),
                Length = length
            };
        });
    }
}
'@
if (-not ('P028SafeEvidenceReader' -as [type])) { Add-Type -TypeDefinition $safeReadType }

function Assert-P028LocalAccount([Security.Principal.SecurityIdentifier]$Sid, [string]$NamePattern) {
    try { $name = $Sid.Translate([Security.Principal.NTAccount]).Value }
    catch { throw 'P028_ROLE_LOCAL_ACCOUNT_UNAVAILABLE' }
    $parts = $name.Split('\')
    if ($parts.Count -ne 2 -or $parts[0] -ine [Environment]::MachineName -or $parts[1] -cnotmatch $NamePattern) {
        throw 'P028_ROLE_LOCAL_ACCOUNT_REQUIRED'
    }
}

function Get-P028ObserverRights([string]$Access) {
    switch -CaseSensitive ($Access) {
        'Read' { return [Security.AccessControl.FileSystemRights]::ReadAndExecute }
        'Write' { return [Security.AccessControl.FileSystemRights]::Modify }
        default { throw 'P028_OBSERVER_ACL_POLICY_INVALID' }
    }
}

function Assert-P028AccessMask([uint32]$Granted, [uint32]$Required, [uint32]$Forbidden) {
    if (($Granted -band $Required) -ne $Required -or ($Granted -band $Forbidden) -ne 0) {
        throw 'P028_ROLE_EFFECTIVE_ACCESS_MISMATCH'
    }
}

function Assert-P028RoleAccess {
    if (-not $ObserverProcessId -or -not $SubjectProcessId -or $ObserverProcessId -eq $SubjectProcessId) {
        throw 'P028_ROLE_PROCESS_REFERENCES_REQUIRED'
    }
    $full = 0x1F01FF
    $read = 0x1200A9
    $modify = 0x1301BF
    # Data/attribute writes, DELETE_CHILD, DELETE, WRITE_DAC and WRITE_OWNER.
    $write = 0x0D0156
    $baselinePath = Join-Path $trustedPath 'P028-LOGON-TASK-BASELINE-v1.json'
    $policies = @(
        @{Path=$root; Observer=$read; Subject=0},
        @{Path=$runRoot; Observer=$read; Subject=$read},
        @{Path=$trustedPath; Observer=$read; Subject=0},
        @{Path=$statePath; Observer=$read; Subject=0},
        @{Path=$observerInbox; Observer=$modify; Subject=0},
        @{Path=$inboxPath; Observer=0; Subject=$modify},
        @{Path=$evidenceRoot; Observer=0; Subject=0}
    )
    if (Test-Path -LiteralPath $baselinePath) { $policies += @{Path=$baselinePath; Observer=$read; Subject=0} }
    foreach ($entry in $policies) {
        [void](Assert-NoReparsePath $entry.Path)
        $descriptor = (Get-Acl -LiteralPath $entry.Path).GetSecurityDescriptorBinaryForm()
        Assert-P028AccessMask ([P028ObserverAccess]::MaximumAccess($PID, $identity.User.Value, 'Controller', $descriptor)) $full 0
        $observerAccess = [P028ObserverAccess]::MaximumAccess($ObserverProcessId, $observer.Value, 'Observer', $descriptor)
        $subjectAccess = [P028ObserverAccess]::MaximumAccess($SubjectProcessId, $subject.Value, 'Subject', $descriptor)
        Assert-P028AccessMask $observerAccess $entry.Observer ($full -bxor $entry.Observer)
        Assert-P028AccessMask $subjectAccess $entry.Subject ($full -bxor $entry.Subject)
    }
    # Parent DELETE_CHILD or ACL/owner/attribute control can bypass a safe child DACL.
    # Parent create-child rights alone do not permit replacing an existing directory.
    for ($parent = [IO.Directory]::GetParent($root); $null -ne $parent; $parent = $parent.Parent) {
        $descriptor = (Get-Acl -LiteralPath $parent.FullName).GetSecurityDescriptorBinaryForm()
        $forbidden = $write -band (-bnot 0x00000006)
        Assert-P028AccessMask ([P028ObserverAccess]::MaximumAccess($ObserverProcessId, $observer.Value, 'Observer', $descriptor)) 0 $forbidden
        Assert-P028AccessMask ([P028ObserverAccess]::MaximumAccess($SubjectProcessId, $subject.Value, 'Subject', $descriptor)) 0 $forbidden
    }
}

function Assert-P028CanonicalPath([string]$Path) {
    if ($Path -notmatch '^[a-z]:\\[^<>:"|?*\x00-\x1f/]+$' -or
        $Path.Substring(3) -match '(?:^|\\)(?:[^\\]*[ .]|(?:CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(?:\.[^\\]*)?)(?:\\|$)') {
        throw 'P028_PATH_NOT_CANONICAL'
    }
    $full = [IO.Path]::GetFullPath($Path)
    if ($full -ine $Path -or $Path.EndsWith('\')) { throw 'P028_PATH_NOT_CANONICAL' }
    return $full
}
