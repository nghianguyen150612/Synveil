[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$InventoryPath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedInventorySha256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedContextSha256
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'P028_WINDOWS_REQUIRED' }
# Read-only diagnostics, never authority. Caller must independently protect and
# authenticate the pre-effect inventory. No resource is deleted by this tool.
$item = Get-Item -LiteralPath $InventoryPath -Force
if ($item.Length -gt 65536 -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'P028_CLEANUP_INVENTORY_UNSAFE'
}
$bytes = [IO.File]::ReadAllBytes($item.FullName)
$sha = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
if ($sha -cne $ExpectedInventorySha256) { throw 'P028_CLEANUP_INVENTORY_CHANGED' }
$inventory = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
if ($inventory.context_sha256 -cne $ExpectedContextSha256 -or
    @($inventory.resources).Count -lt 9 -or @($inventory.resources).Count -gt 256) {
    throw 'P028_CLEANUP_CONTEXT_INVALID'
}
$kinds = @('account','profile','installation','task','process','filesystem','secret','protected_state','pending_operation')
foreach ($kind in $kinds) {
    if (@($inventory.resources | Where-Object { $_.kind -ceq $kind }).Count -eq 0) {
        throw 'P028_CLEANUP_INVENTORY_INCOMPLETE'
    }
}
$results = @()
foreach ($r in $inventory.resources) {
    $status = 'UNKNOWN'
    try {
        if ($r.kind -notin $kinds -or $r.disposition -notin @('DELETE','RETAIN_PROTECTED')) {
            throw 'P028_CLEANUP_RESOURCE_INVALID'
        }
        if ($r.disposition -ceq 'DELETE') {
            switch -CaseSensitive ($r.kind) {
                'account' {
                    if ($r.identity -cnotmatch '^S-1-5-21-\d+-\d+-\d+-\d+$') { throw 'SID_INVALID' }
                    $found = @(Get-CimInstance Win32_UserAccount -Filter "SID='$($r.identity)'" -ErrorAction Stop)
                    $status = if ($found.Count -eq 0) { 'ABSENT' } else { 'PRESENT' }
                }
                'profile' {
                    if ($r.identity -cnotmatch '^S-1-5-21-\d+-\d+-\d+-\d+$') { throw 'SID_INVALID' }
                    $found = @(Get-CimInstance Win32_UserProfile -Filter "SID='$($r.identity)'" -ErrorAction Stop)
                    $status = if ($found.Count -eq 0) { 'ABSENT' } else { 'PRESENT' }
                }
                'process' {
                    if ($r.identity -cnotmatch '^[1-9][0-9]{0,9}$') { throw 'PID_INVALID' }
                    $found = @(Get-CimInstance Win32_Process -Filter "ProcessId=$($r.identity)" -ErrorAction Stop)
                    # A reused PID blocks conservatively; never terminate it.
                    $status = if ($found.Count -eq 0) { 'ABSENT' } else { 'PRESENT' }
                }
                'task' {
                    # Enumerate and compare literally; no wildcard selection.
                    $found = @(Get-ScheduledTask -ErrorAction Stop | Where-Object {
                        ($_.TaskPath + $_.TaskName) -ceq $r.identity
                    })
                    $status = if ($found.Count -eq 0) { 'ABSENT' } else { 'PRESENT' }
                }
                { $_ -in @('filesystem','installation') } {
                    if (-not [IO.Path]::IsPathFullyQualified([string]$r.identity)) { throw 'PATH_INVALID' }
                    # Get-Item distinguishes missing leaf from denied/unsupported query.
                    try {
                        $null = Get-Item -LiteralPath $r.identity -Force -ErrorAction Stop
                        $status = 'PRESENT'
                    } catch [System.Management.Automation.ItemNotFoundException] {
                        # A missing/denied ancestor cannot establish absence securely.
                        $null = Get-Item -LiteralPath ([IO.Path]::GetDirectoryName($r.identity)) -Force -ErrorAction Stop
                        $status = 'ABSENT'
                    }
                }
                # Credentials, protected retained audit state and pending operations
                # need authority-owned enumerators. Never infer absence from an exit code.
                default { $status = 'UNKNOWN' }
            }
        }
    } catch { $status = 'UNKNOWN' }
    $refBytes = [Text.Encoding]::UTF8.GetBytes(([string]$r.kind + ':' + [string]$r.identity))
    $reference = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($refBytes)).ToLowerInvariant()
    $results += [ordered]@{kind=$r.kind;reference_sha256=$reference;status=$status}
}
[ordered]@{
    mode='NATIVE_DIAGNOSTIC_ONLY';context_sha256=$ExpectedContextSha256
    inventory_sha256=$sha;resources=$results;cleanup='CLEANUP_BLOCKED'
    authority='P028_NATIVE_AUTHORITY_UNAVAILABLE'
} | ConvertTo-Json -Depth 5
