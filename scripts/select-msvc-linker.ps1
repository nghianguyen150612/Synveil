$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($env:VCToolsInstallDir) -or
    [string]::IsNullOrWhiteSpace($env:VCToolsRedistDir) -or
    [string]::IsNullOrWhiteSpace($env:VCToolsVersion)) {
  throw 'MSVC_TOOLCHAIN_SELECTION_FAILURE: the active developer environment omitted its tools, redistributable, or version identity'
}

$toolsRoot = [IO.Path]::GetFullPath($env:VCToolsInstallDir).TrimEnd('\') + '\'
$linker = [IO.Path]::GetFullPath((Join-Path $toolsRoot 'bin\Hostx64\x64\link.exe'))
if (!$linker.StartsWith($toolsRoot, [StringComparison]::OrdinalIgnoreCase) -or
    !(Test-Path -LiteralPath $linker -PathType Leaf)) {
  throw 'MSVC_LINKER_SELECTION_FAILURE: the selected linker is outside the active Visual C++ tools root or missing'
}
$linkerFile = Get-Item -LiteralPath $linker
if ($null -ne $linkerFile.LinkType) {
  throw 'MSVC_LINKER_SELECTION_FAILURE: the selected linker must be a regular, non-reparse-point file'
}
$linkerIdentity = [Diagnostics.FileVersionInfo]::GetVersionInfo($linker)
if ($linkerIdentity.CompanyName -notmatch '^Microsoft Corporation$' -or
    $linkerIdentity.OriginalFilename -notmatch '^LINK\.EXE$') {
  throw "MSVC_LINKER_SELECTION_FAILURE: unexpected linker identity company='$($linkerIdentity.CompanyName)' original='$($linkerIdentity.OriginalFilename)'"
}
$linkerBytes = [IO.File]::ReadAllBytes($linker)
if ($linkerBytes.Length -lt 512 -or $linkerBytes[0] -ne 0x4d -or $linkerBytes[1] -ne 0x5a) {
  throw 'MSVC_LINKER_SELECTION_FAILURE: the selected linker is not a PE executable'
}
$peOffset = [BitConverter]::ToInt32($linkerBytes, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 6 -ge $linkerBytes.Length -or
    [BitConverter]::ToUInt32($linkerBytes, $peOffset) -ne 0x00004550 -or
    [BitConverter]::ToUInt16($linkerBytes, $peOffset + 4) -ne 0x8664) {
  throw 'MSVC_LINKER_SELECTION_FAILURE: the selected linker is not an AMD64 PE executable'
}

$redistRoot = [IO.Path]::GetFullPath($env:VCToolsRedistDir).TrimEnd('\')
if ([IO.Path]::GetFileName($redistRoot) -cne $env:VCToolsVersion) {
  throw 'MSVC_CRT_SELECTION_FAILURE: the active redistributable version does not match VCToolsVersion'
}
$x64RedistRoot = Join-Path $redistRoot 'x64'
if (!(Test-Path -LiteralPath $x64RedistRoot -PathType Container)) {
  throw 'MSVC_CRT_SELECTION_FAILURE: the active x64 redistributable directory is missing'
}
$crtCandidates = @(Get-ChildItem -LiteralPath $x64RedistRoot -Directory -Filter 'Microsoft.VC*.CRT' |
  Where-Object {
    (Test-Path -LiteralPath (Join-Path $_.FullName 'msvcp140.dll') -PathType Leaf) -and
    (Test-Path -LiteralPath (Join-Path $_.FullName 'vcruntime140.dll') -PathType Leaf)
  })
if ($crtCandidates.Count -ne 1) {
  $candidateNames = ($crtCandidates | ForEach-Object { $_.Name }) -join ', '
  throw "MSVC_CRT_SELECTION_FAILURE: expected one x64 MSVC CRT directory under the active redist, found $($crtCandidates.Count): $candidateNames"
}
$crtDirectory = [IO.Path]::GetFullPath($crtCandidates[0].FullName)
$crtRoot = $redistRoot.TrimEnd('\') + '\'
if (!$crtDirectory.StartsWith($crtRoot, [StringComparison]::OrdinalIgnoreCase) -or
    !(Test-Path -LiteralPath $crtDirectory -PathType Container)) {
  throw 'MSVC_CRT_SELECTION_FAILURE: the selected x64 redistributable directory is outside VCToolsRedistDir'
}
$crtDirectoryItem = Get-Item -LiteralPath $crtDirectory
if ($null -ne $crtDirectoryItem.LinkType) {
  throw 'MSVC_CRT_SELECTION_FAILURE: the active x64 VC143 redistributable directory must not be a reparse point'
}
foreach ($name in @('msvcp140.dll', 'vcruntime140.dll')) {
  $runtime = Join-Path $crtDirectory $name
  if (!(Test-Path -LiteralPath $runtime -PathType Leaf)) {
    throw "MSVC_CRT_SELECTION_FAILURE: required runtime DLL is missing: $name"
  }
  $runtimeFile = Get-Item -LiteralPath $runtime
  $runtimeIdentity = [Diagnostics.FileVersionInfo]::GetVersionInfo($runtime)
  if ($null -ne $runtimeFile.LinkType -or $runtimeIdentity.CompanyName -notmatch '^Microsoft Corporation$') {
    throw "MSVC_CRT_SELECTION_FAILURE: unexpected runtime DLL identity: $name"
  }
}

"CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=$linker" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
"SYNVEIL_MSVC_CRT_DIR=$crtDirectory" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
Write-Host "Authenticated MSVC linker: $linker"
Write-Host "MSVC toolset version: $env:VCToolsVersion"
Write-Host "Linker SHA-256: $((Get-FileHash -LiteralPath $linker -Algorithm SHA256).Hash.ToLowerInvariant())"
Write-Host "Authenticated MSVC CRT directory: $crtDirectory"
foreach ($name in @('msvcp140.dll', 'vcruntime140.dll')) {
  Write-Host "$name SHA-256: $((Get-FileHash -LiteralPath (Join-Path $crtDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant())"
}
