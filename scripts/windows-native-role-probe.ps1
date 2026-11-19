[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('Observer','Subject')][string]$Role,
    [Parameter(Mandatory=$true)][string]$ExpectedSid,
    [Parameter(Mandatory=$true)][string]$ObserverInbox,
    [Parameter(Mandatory=$true)][string]$SubjectInbox,
    [Parameter(Mandatory=$true)][string]$EvidenceFile,
    [Parameter(Mandatory=$true)][string]$ProtectedStateFile,
    [Parameter(Mandatory=$true)][string]$Nonce,
    [Parameter(Mandatory=$true)][string]$ReleaseFile
)

$ErrorActionPreference = 'Stop'
$ErrorCode = 10
$native = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class P028RoleProbeNative {
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out SafeAccessTokenHandle token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(SafeAccessTokenHandle token, int kind, out int value, int size, out int needed);
    public static bool IsElevated() {
        SafeAccessTokenHandle token;
        if (!OpenProcessToken(GetCurrentProcess(), 0x0008, out token)) throw new Win32Exception();
        using (token) {
            int value, needed;
            if (!GetTokenInformation(token, 20, out value, sizeof(int), out needed)) throw new Win32Exception();
            return value != 0;
        }
    }
}
'@
if (-not ('P028RoleProbeNative' -as [type])) { Add-Type -TypeDefinition $native }

function Assert-WriteDenied([string]$Path) {
    try {
        [IO.File]::WriteAllText($Path, 'UNAUTHORIZED-' + $Nonce)
        throw 'P028_ROLE_WRITE_WAS_ALLOWED'
    } catch [UnauthorizedAccessException] {
        return
    } catch [Security.SecurityException] {
        return
    }
}
function Assert-CreateDenied([string]$Directory, [string]$Name) {
    $path = Join-Path $Directory $Name
    try {
        [IO.File]::WriteAllText($path, 'UNAUTHORIZED-' + $Nonce)
        throw 'P028_ROLE_CREATE_WAS_ALLOWED'
    } catch [UnauthorizedAccessException] {
        return
    } catch [Security.SecurityException] {
        return
    }
}
function Assert-CreateWritable([string]$Directory, [string]$Name, [string]$Expected) {
    $path = Join-Path $Directory $Name
    [IO.File]::WriteAllText($path, $Expected)
    if ([IO.File]::ReadAllText($path) -cne $Expected) { throw 'P028_ROLE_WRITE_MISMATCH' }
}
function Assert-Readable([string]$Path, [string]$Expected) {
    $actual = [IO.File]::ReadAllText($Path)
    if ($actual -cne $Expected) { throw 'P028_ROLE_READ_MISMATCH' }
}
function Assert-ReadDenied([string]$Path) {
    try {
        [void][IO.File]::ReadAllText($Path)
        throw 'P028_ROLE_READ_WAS_ALLOWED'
    } catch [UnauthorizedAccessException] {
        return
    } catch [Security.SecurityException] {
        return
    }
}
function Assert-Writable([string]$Path, [string]$Expected) {
    [IO.File]::WriteAllText($Path, $Expected)
    if ([IO.File]::ReadAllText($Path) -cne $Expected) { throw 'P028_ROLE_WRITE_MISMATCH' }
}

try {
    . (Join-Path $PSScriptRoot 'windows-native-observer-support.ps1')
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        if ($identity.User.Value -cne $ExpectedSid) { throw 'P028_ROLE_TOKEN_SID_MISMATCH' }
        if ([P028RoleProbeNative]::IsElevated() -or
            [Security.Principal.WindowsPrincipal]::new($identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw 'P028_ROLE_TOKEN_PRIVILEGED'
        }
        $isObserver = $Role -ceq 'Observer'
        [P028ObserverAccess]::AssertRoleToken([uint32]$PID, $ExpectedSid, $isObserver)
        if ($isObserver) {
            # Read one real record from each required event channel while this
            # process holds the observer token. Empty or inaccessible channels
            # fail closed; no event contents are retained in evidence.
            [void](Get-WinEvent -LogName 'Security' -MaxEvents 1 -ErrorAction Stop)
            [void](Get-WinEvent -LogName 'Microsoft-Windows-TaskScheduler/Operational' -MaxEvents 1 -ErrorAction Stop)
            Assert-Readable $EvidenceFile ('EVIDENCE-' + $Nonce)
            Assert-CreateWritable $ObserverInbox ($Nonce + '.json') ('OBSERVER-' + $Nonce)
            Assert-WriteDenied $EvidenceFile
            Assert-WriteDenied $ProtectedStateFile
            Assert-CreateDenied $SubjectInbox ('blocked-' + $Nonce + '.txt')
        } else {
            Assert-Readable (Join-Path $SubjectInbox 'existing.txt') ('SUBJECT-SEED-' + $Nonce)
            Assert-CreateWritable $SubjectInbox ($Nonce + '.txt') ('SUBJECT-' + $Nonce)
            Assert-WriteDenied $ProtectedStateFile
            Assert-CreateDenied $ObserverInbox ('blocked-' + $Nonce + '.txt')
            Assert-CreateDenied (Split-Path -Parent $EvidenceFile) ('blocked-' + $Nonce + '.txt')
            Assert-ReadDenied $EvidenceFile
        }
        $deadline = [DateTimeOffset]::UtcNow.AddSeconds(25)
        while (-not [IO.File]::Exists($ReleaseFile) -and [DateTimeOffset]::UtcNow -lt $deadline) {
            Start-Sleep -Milliseconds 100
        }
        if (-not [IO.File]::Exists($ReleaseFile)) { throw 'P028_ROLE_PROBE_RELEASE_TIMEOUT' }
    } finally { $identity.Dispose() }
    exit 0
} catch {
    # Only stable reason codes are emitted; exception text can include a path or account.
    $reason = $_.Exception.Message
    if ($reason -notmatch '^P028_[A-Z0-9_]+$') { $reason = 'P028_ROLE_PROBE_OPERATION_FAILED' }
    [Console]::Error.WriteLine($reason)
    exit 20
}
