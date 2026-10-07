# P043 shared path checks for the existing Windows producer and native validators.
Set-StrictMode -Version Latest
function Assert-SafeRelativeIdentity([string]$Value) {
    if ([string]::IsNullOrEmpty($Value) -or $Value.Length -gt 240 -or
        $Value -match '[\x00-\x1f\x7f-\x9f:<>"|?*{}]' -or [IO.Path]::IsPathRooted($Value)) {
        throw 'WINDOWS_PATH_IDENTITY_FAILURE: unsafe relative identity'
    }
    foreach ($part in $Value.Replace('\','/').Split('/')) {
        if (!$part -or $part -in @('.','..') -or $part -cne $part.Trim() -or $part.EndsWith('.') -or
            $part.Split('.')[0] -match '^(?i:CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9]|LPT[1-9])$') {
            throw 'WINDOWS_PATH_IDENTITY_FAILURE: unsafe path component'
        }
    }
}
function Assert-NoReparseAncestry([string]$Path, [bool]$RegularFile = $false) {
    $current = [IO.Path]::GetFullPath($Path)
    $first = $true
    while ($current) {
        # Get-Item inspects the reparse object itself; do not resolve it first.
        $item = Get-Item -LiteralPath $current -Force -ErrorAction SilentlyContinue
        if ($null -ne $item) {
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
                ($first -and $RegularFile -and $item.PSIsContainer) -or
                ((!$first -or !$RegularFile) -and !$item.PSIsContainer)) {
                throw 'WINDOWS_PATH_IDENTITY_FAILURE: ambiguous filesystem ancestry'
            }
        } elseif (Test-Path -LiteralPath $current -ErrorAction Stop) {
            throw 'WINDOWS_PATH_IDENTITY_FAILURE: uninspectable filesystem object'
        }
        $first = $false
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
function Get-OwnedRegisteredUninstaller([string]$Command, [string]$Root) {
    # Registration supplies one executable identity, never executable syntax.
    if ($Command -notmatch '^"([^"\r\n]+)"(?:\s+/SILENT)?$') {
        throw 'WINDOWS_UNINSTALL_IDENTITY_FAILURE: unsupported registered command'
    }
    $executable = [IO.Path]::GetFullPath($Matches[1])
    $relative = [IO.Path]::GetRelativePath([IO.Path]::GetFullPath($Root), $executable)
    Assert-SafeRelativeIdentity $relative
    if ($relative -notmatch '^unins[0-9]{3}\.exe$') {
        throw 'WINDOWS_UNINSTALL_IDENTITY_FAILURE: unproven uninstaller identity'
    }
    Assert-NoReparseAncestry $executable $true
    if (!(Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw 'WINDOWS_UNINSTALL_IDENTITY_FAILURE: uninstaller is absent'
    }
    return $executable
}
