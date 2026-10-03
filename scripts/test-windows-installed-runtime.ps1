param(
    [Parameter(Mandatory=$true)][string]$RuntimeRoot,
    [Parameter(Mandatory=$true)][string]$Manifest
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($RuntimeRoot).TrimEnd([IO.Path]::DirectorySeparatorChar)
$manifestPath = [IO.Path]::GetFullPath($Manifest)
if (!(Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'RUNTIME_MANIFEST_FAILURE: manifest is missing' }

$seen = @{}
$inFiles = $false
foreach ($line in [IO.File]::ReadAllLines($manifestPath)) {
    if ($line -ceq 'files=sha256 size path') { $inFiles = $true; continue }
    if (!$inFiles) { continue }
    if ($line -notmatch '^([0-9a-f]{64}) ([0-9]+) (.+)$') { throw 'RUNTIME_MANIFEST_FAILURE: malformed inventory entry' }
    $relative = $Matches[3].Replace('\','/')
    if ([IO.Path]::IsPathRooted($relative) -or $relative.Split('/') -contains '..') { throw 'RUNTIME_MANIFEST_FAILURE: unsafe inventory path' }
    $key = $relative.ToLowerInvariant()
    if ($seen.ContainsKey($key)) { throw 'RUNTIME_MANIFEST_FAILURE: duplicate or case-colliding path' }
    $seen[$key] = $true
    $file = [IO.Path]::GetFullPath((Join-Path $root $relative))
    if (!$file.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'RUNTIME_MANIFEST_FAILURE: inventory path escaped package root' }
    if (!(Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).LinkType) { throw "RUNTIME_MANIFEST_FAILURE: missing or linked file $relative" }
    if ((Get-Item -LiteralPath $file).Length -ne [int64]$Matches[2] -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Matches[1]) { throw "RUNTIME_MANIFEST_FAILURE: size or digest mismatch $relative" }
}
foreach ($required in @('synveil-desktop.exe','synveil-client.exe','qt.conf','platforms/qwindows.dll','LICENSE','NOTICE')) {
    if (!$seen.ContainsKey($required.ToLowerInvariant())) { throw "RUNTIME_MANIFEST_FAILURE: required file is not bound: $required" }
}
$actual = @(Get-ChildItem -LiteralPath $root -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($root,$_.FullName).Replace('\','/') } | Where-Object { $_ -notin @('SYNVEIL-MANIFEST.txt','unins000.exe','unins000.dat','unins000.msg') })
$unexpected = @($actual | Where-Object { !$seen.ContainsKey($_.ToLowerInvariant()) })
if ($unexpected.Count) { throw "RUNTIME_MANIFEST_FAILURE: unmanifested package file: $($unexpected[0])" }
if ($actual.Count -ne $seen.Count) { throw 'RUNTIME_MANIFEST_FAILURE: installed inventory count mismatch' }

foreach ($binaryName in @('synveil-desktop.exe','synveil-client.exe')) {
    $bytes = [IO.File]::ReadAllBytes((Join-Path $root $binaryName))
    if ($bytes.Length -lt 512 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw "RUNTIME_ARCH_FAILURE: $binaryName is not PE" }
    $pe = [BitConverter]::ToInt32($bytes,0x3c)
    if ($pe -lt 0 -or $pe + 6 -ge $bytes.Length -or [BitConverter]::ToUInt32($bytes,$pe) -ne 0x00004550 -or [BitConverter]::ToUInt16($bytes,$pe+4) -ne 0x8664) { throw "RUNTIME_ARCH_FAILURE: $binaryName is not AMD64" }
}
Write-Host "Installed runtime manifest: PASS ($($seen.Count) bound files)"
