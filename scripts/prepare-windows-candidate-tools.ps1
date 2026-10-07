# Select build tools by their active Visual C++ installation, never PATH order.
param([string]$EvidencePath = 'target/windows-toolchain.json')
$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($env:VCToolsInstallDir)) { throw 'MSVC_TOOLS_UNAVAILABLE' }
$toolsRoot = [IO.Path]::GetFullPath($env:VCToolsInstallDir).TrimEnd('\') + '\'
function Get-ToolIdentity([string]$Name) {
    $path = [IO.Path]::GetFullPath((Join-Path $toolsRoot "bin\Hostx64\x64\$Name"))
    $file = Get-Item -LiteralPath $path
    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($path)
    if (!$path.StartsWith($toolsRoot, [StringComparison]::OrdinalIgnoreCase) -or $null -ne $file.LinkType -or $version.CompanyName -cne 'Microsoft Corporation' -or $version.OriginalFilename -ine $Name) { throw "MSVC_TOOL_IDENTITY_FAILURE: $Name" }
    $bytes = [IO.File]::ReadAllBytes($path)
    if ($bytes.Length -lt 512 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw "MSVC_PE_FAILURE: $Name" }
    $offset = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($offset -lt 0 -or $offset + 6 -ge $bytes.Length -or [BitConverter]::ToUInt32($bytes, $offset) -ne 0x00004550 -or [BitConverter]::ToUInt16($bytes, $offset + 4) -ne 0x8664) { throw "MSVC_ARCHITECTURE_FAILURE: $Name" }
    return [ordered]@{path=$path;version=$version.FileVersion;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
}
$linker = Get-ToolIdentity 'link.exe'
$compiler = Get-ToolIdentity 'cl.exe'
$redistRoot = $env:VCToolsRedistDir
if ([string]::IsNullOrWhiteSpace($redistRoot)) {
    $redistVersion = (Get-Content (Join-Path $env:VCINSTALLDIR 'Auxiliary\Build\Microsoft.VCRedistVersion.default.txt') -Raw).Trim()
    $redistRoot = Join-Path $env:VCINSTALLDIR "Redist\MSVC\$redistVersion"
}
$crt = [IO.Path]::GetFullPath((Join-Path $redistRoot 'x64\Microsoft.VC143.CRT'))
$runtime = @(Get-ChildItem -LiteralPath $crt -Filter '*.dll' -File)
if ($runtime.Count -eq 0 -or !(Test-Path (Join-Path $crt 'msvcp140.dll'))) { throw 'MSVC_CRT_UNAVAILABLE' }
$dllIdentities = @($runtime | ForEach-Object {
    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($_.FullName)
    if ($null -ne $_.LinkType -or $version.CompanyName -cne 'Microsoft Corporation') { throw "MSVC_CRT_IDENTITY_FAILURE: $($_.Name)" }
    [ordered]@{filename=$_.Name;version=$version.FileVersion;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
})
"CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=$($linker.path)" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append
"SYNVEIL_MSVC_CRT_DIR=$crt" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append
New-Item (Split-Path $EvidencePath -Parent) -ItemType Directory -Force | Out-Null
[ordered]@{schema_version=1;compiler=$compiler;linker=$linker;msvc_tools=$env:VCToolsVersion;runtime=$dllIdentities} | ConvertTo-Json -Depth 6 | Set-Content $EvidencePath -Encoding utf8
Write-Host "Selected MSVC compiler $($compiler.version), linker $($linker.version); app-local CRT files: $($runtime.Count)"
