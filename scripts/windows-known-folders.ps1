# Resolve the current process token's Windows namespace even when its process
# environment was inherited from the runner account. Passing the token is
# required: the hosted -Credential child otherwise resolved the runner's
# interactive profile. Do not create folders.
if (-not ('SynveilKnownFolders' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class SynveilKnownFolders {
    private const uint TOKEN_DUPLICATE = 0x0002;
    private const uint TOKEN_IMPERSONATE = 0x0004;
    private const uint TOKEN_QUERY = 0x0008;
    private const uint KF_FLAG_DONT_VERIFY = 0x00004000;

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("advapi32.dll", SetLastError=true)]
    private static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);

    [DllImport("userenv.dll", EntryPoint="GetUserProfileDirectoryW", ExactSpelling=true, SetLastError=true)]
    private static extern bool GetUserProfileDirectory(IntPtr token, IntPtr path, ref uint size);

    [DllImport("shell32.dll", EntryPoint="SHGetKnownFolderPath", ExactSpelling=true)]
    private static extern int SHGetKnownFolderPath(ref Guid folder, uint flags, IntPtr token, out IntPtr path);

    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool CloseHandle(IntPtr handle);

    public static string CurrentUserProfile() {
        IntPtr token;
        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, out token)) {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        try {
            uint size = 0;
            GetUserProfileDirectory(token, IntPtr.Zero, ref size);
            int error = Marshal.GetLastWin32Error();
            if (size == 0 || error != 122) {
                throw new Win32Exception(error, "GetUserProfileDirectory did not provide a profile path size");
            }
            IntPtr path = Marshal.AllocHGlobal(checked((int)size * 2));
            try {
                if (!GetUserProfileDirectory(token, path, ref size)) {
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                }
                return Marshal.PtrToStringUni(path);
            }
            finally {
                Marshal.FreeHGlobal(path);
            }
        }
        finally {
            CloseHandle(token);
        }
    }

    public static string Resolve(string folderId) {
        Guid folder = new Guid(folderId);
        IntPtr token;
        if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE, out token)) {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        IntPtr path = IntPtr.Zero;
        try {
            int result = SHGetKnownFolderPath(ref folder, KF_FLAG_DONT_VERIFY, token, out path);
            Marshal.ThrowExceptionForHR(result);
            return Marshal.PtrToStringUni(path);
        }
        finally {
            if (path != IntPtr.Zero) Marshal.FreeCoTaskMem(path);
            CloseHandle(token);
        }
    }
}
'@
}

function Get-WindowsKnownFolderPath([string]$Folder) {
    $folderIds = @{
        LocalApplicationData = 'F1B32785-6FBA-4FCF-9D55-7B8E7F157091'
        Programs = 'A77F5D77-2E2B-44C3-A6A2-ABA601054A51'
        DesktopDirectory = 'B4BFCC3A-DB2C-424C-B029-7FE99A87C641'
        CommonPrograms = '0139D44E-6AFE-49F2-8690-3DAFCAE6FFB8'
        CommonDesktopDirectory = 'C4AA340D-F20F-4863-AFEF-F87EF2E6BA25'
    }
    if (!$folderIds.ContainsKey($Folder)) {
        throw "WINDOWS_PROFILE_FAILURE: unsupported Windows known folder $Folder"
    }
    # The hosted -Credential child returned the launcher profile from
    # SHGetKnownFolderPath. Resolve user-scoped folders from the process token's
    # GetUserProfileDirectory result instead; the caller separately verifies
    # that this token belongs to the disposable non-administrator account.
    $profile = [SynveilKnownFolders]::CurrentUserProfile()
    $path = switch ($Folder) {
        'LocalApplicationData' { Join-Path $profile 'AppData\Local'; break }
        'Programs' { Join-Path $profile 'AppData\Roaming\Microsoft\Windows\Start Menu\Programs'; break }
        'DesktopDirectory' { Join-Path $profile 'Desktop'; break }
        default { [SynveilKnownFolders]::Resolve($folderIds[$Folder]) }
    }
    if ([string]::IsNullOrWhiteSpace($path) -or ![IO.Path]::IsPathFullyQualified($path)) {
        throw "WINDOWS_PROFILE_FAILURE: Windows known folder $Folder is unavailable"
    }
    return $path
}

function Set-WindowsTokenProfileEnvironment {
    # -Credential loads the token's profile but inherits the launcher environment.
    # Setup and its temporary extraction must use the verified child's namespace.
    $profileRoot = [SynveilKnownFolders]::CurrentUserProfile()
    $localAppData = Join-Path $profileRoot 'AppData\Local'
    $tokenTemp = Join-Path $localAppData 'Temp'
    New-Item $tokenTemp -ItemType Directory -Force | Out-Null
    $profileDrive = Split-Path -Qualifier $profileRoot
    $env:USERPROFILE = $profileRoot
    $env:LOCALAPPDATA = $localAppData
    $env:APPDATA = Join-Path $profileRoot 'AppData\Roaming'
    $env:HOMEDRIVE = $profileDrive
    $env:HOMEPATH = $profileRoot.Substring($profileDrive.Length)
    $env:TEMP = $tokenTemp
    $env:TMP = $tokenTemp
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $env:USERNAME = $identity.Name.Substring($identity.Name.LastIndexOf('\') + 1)
    $env:USERDOMAIN = $env:COMPUTERNAME
}
