# Resolve the current process token's Windows namespace even when its process
# environment was inherited from the runner account. Do not create folders.
if (-not ('SynveilKnownFolders' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SynveilKnownFolders {
    [DllImport("shell32.dll", EntryPoint="SHGetKnownFolderPath", ExactSpelling=true)]
    private static extern int SHGetKnownFolderPath(ref Guid folder, uint flags, IntPtr token, out IntPtr path);

    public static string Resolve(string folderId) {
        Guid folder = new Guid(folderId);
        IntPtr path;
        int result = SHGetKnownFolderPath(ref folder, 0x4000, IntPtr.Zero, out path); // KF_FLAG_DONT_VERIFY; current process token
        Marshal.ThrowExceptionForHR(result);
        try { return Marshal.PtrToStringUni(path); }
        finally { Marshal.FreeCoTaskMem(path); }
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
    $path = [SynveilKnownFolders]::Resolve($folderIds[$Folder])
    if ([string]::IsNullOrWhiteSpace($path) -or ![IO.Path]::IsPathFullyQualified($path)) {
        throw "WINDOWS_PROFILE_FAILURE: Windows known folder $Folder is unavailable"
    }
    return $path
}
