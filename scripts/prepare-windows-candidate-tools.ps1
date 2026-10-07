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
# NCrypt is a Windows CNG system component since Vista, including Windows 11.
# Observe only the OS-selected System32 file, never a DLL found through PATH or
# the payload. This producer observation does not qualify a Windows 11 journey.
$systemDirectory = [Environment]::SystemDirectory
$ncryptPath = [IO.Path]::GetFullPath((Join-Path $systemDirectory 'ncrypt.dll'))
$ncryptFile = Get-Item -LiteralPath $ncryptPath
$ncryptVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($ncryptPath)
$ncryptSignature = Get-AuthenticodeSignature -LiteralPath $ncryptPath
New-Item (Split-Path $EvidencePath -Parent) -ItemType Directory -Force | Out-Null
$ncryptObservation = [ordered]@{name='ncrypt.dll';path=$ncryptPath;company=$ncryptVersion.CompanyName;original_filename=$ncryptVersion.OriginalFilename;signature_status=[string]$ncryptSignature.Status;signer=$ncryptSignature.SignerCertificate.Subject;version=$ncryptVersion.FileVersion;sha256=(Get-FileHash -LiteralPath $ncryptPath -Algorithm SHA256).Hash.ToLowerInvariant()}
[ordered]@{schema_version=1;status='preflight';source_commit=(git rev-parse HEAD);compiler=$compiler;linker=$linker;system_dlls=@($ncryptObservation)} | ConvertTo-Json -Depth 6 | Set-Content $EvidencePath -Encoding utf8
Write-Host ("Observed OS NCrypt identity: " + ($ncryptObservation | ConvertTo-Json -Compress))
# Windows' signed version resource names its localized source ncrypt.dll.mui;
# ownership is established by the fixed OS path, trusted Microsoft signature
# and AMD64 PE identity, not by treating that resource name as a payload path.
if ($null -ne $ncryptFile.LinkType -or $ncryptVersion.CompanyName -cne 'Microsoft Corporation' -or $ncryptVersion.OriginalFilename -inotmatch '^ncrypt\.dll(?:\.mui)?$' -or $ncryptSignature.Status -ne 'Valid' -or $ncryptSignature.SignerCertificate.Subject -notmatch 'CN=Microsoft (Windows|Corporation)(,|$)') { throw 'WINDOWS_SYSTEM_NCRYPT_IDENTITY_FAILURE' }
$ncryptBytes = [IO.File]::ReadAllBytes($ncryptPath)
$ncryptOffset = [BitConverter]::ToInt32($ncryptBytes, 0x3c)
if ($ncryptOffset -lt 0 -or $ncryptOffset + 6 -ge $ncryptBytes.Length -or [BitConverter]::ToUInt32($ncryptBytes, $ncryptOffset) -ne 0x00004550 -or [BitConverter]::ToUInt16($ncryptBytes, $ncryptOffset + 4) -ne 0x8664) { throw 'WINDOWS_SYSTEM_NCRYPT_ARCHITECTURE_FAILURE' }
$systemNcrypt = [ordered]@{name='ncrypt.dll';path=$ncryptPath;owner='Windows OS';signature_status=[string]$ncryptSignature.Status;signer=$ncryptSignature.SignerCertificate.Subject;version=$ncryptVersion.FileVersion;sha256=(Get-FileHash -LiteralPath $ncryptPath -Algorithm SHA256).Hash.ToLowerInvariant()}
$redistRoot = $env:VCToolsRedistDir
if ([string]::IsNullOrWhiteSpace($redistRoot)) {
    $redistVersion = (Get-Content (Join-Path $env:VCINSTALLDIR 'Auxiliary\Build\Microsoft.VCRedistVersion.default.txt') -Raw).Trim()
    $redistRoot = Join-Path $env:VCINSTALLDIR "Redist\MSVC\$redistVersion"
}
$x64Redist = [IO.Path]::GetFullPath((Join-Path $redistRoot 'x64')).TrimEnd('\') + '\'
# The active runner may select VS 2026 rather than VS 2022. Select exactly one
# CRT directory inside that active redist root, without guessing its generation
# or searching a different SDK/System32 installation.
$crtDirectories = @(Get-ChildItem -LiteralPath $x64Redist -Directory -Filter 'Microsoft.VC*.CRT' |
    Where-Object { $_.Name -match '^Microsoft\.VC\d+\.CRT$' })
if ($crtDirectories.Count -ne 1 -or $null -ne $crtDirectories[0].LinkType) { throw 'MSVC_CRT_SELECTION_FAILURE' }
$crt = [IO.Path]::GetFullPath($crtDirectories[0].FullName)
if (!$crt.StartsWith($x64Redist, [StringComparison]::OrdinalIgnoreCase)) { throw 'MSVC_CRT_CONTAINMENT_FAILURE' }
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
$os = Get-CimInstance Win32_OperatingSystem
[ordered]@{schema_version=1;source_commit=(git rev-parse HEAD);compiler=$compiler;linker=$linker;msvc_tools=$env:VCToolsVersion;crt_directory=$crtDirectories[0].Name;runtime=$dllIdentities;system_dlls=@($systemNcrypt);producer_os=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;architecture=$env:PROCESSOR_ARCHITECTURE}} | ConvertTo-Json -Depth 6 | Set-Content $EvidencePath -Encoding utf8
Write-Host "Selected MSVC compiler $($compiler.version), linker $($linker.version); app-local CRT files: $($runtime.Count)"
