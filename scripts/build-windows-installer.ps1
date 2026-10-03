[CmdletBinding()]
param(
    [string]$OutputDirectory = "target/windows-installer",
    [string]$RuntimeStagingDirectory,
    [string]$InnoToolchainDirectory,
    [switch]$Diagnostics
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Fail([string]$Kind, [string]$Message) { throw "${Kind}: ${Message}" }
function Full-RepoPath([string]$Path, [string]$Root) {
    if ([string]::IsNullOrWhiteSpace($Path) -or $Path.IndexOfAny([char[]]"`0`r`n") -ge 0) { Fail "PAYLOAD_IDENTITY_FAILURE" "invalid path" }
    if (![IO.Path]::IsPathRooted($Path)) { $Path = Join-Path $Root $Path }
    return [IO.Path]::GetFullPath($Path)
}
function Invoke-Checked([string]$File, [string[]]$Arguments, [string]$Kind) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { Fail $Kind "process exited $LASTEXITCODE" }
}
function Get-WorkspaceVersion([string]$CargoToml) {
    $text = [IO.File]::ReadAllText($CargoToml)
    $match = [regex]::Match($text, '(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"([^"\r\n]+)"')
    if (!$match.Success) { Fail "PAYLOAD_IDENTITY_FAILURE" "workspace package version is missing" }
    $version = $match.Groups[1].Value
    $semver = [regex]::Match($version, '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$')
    if (!$semver.Success -or $semver.Groups[4].Success -or $semver.Groups[5].Success) {
        Fail "PAYLOAD_IDENTITY_FAILURE" "version '$version' cannot be represented as a Windows version"
    }
    $parts = 1..3 | ForEach-Object { [uint32]::Parse($semver.Groups[$_].Value) }
    if ($parts | Where-Object { $_ -gt 65535 }) { Fail "PAYLOAD_IDENTITY_FAILURE" "version component exceeds 65535" }
    return @{ Product = $version; Windows = "$($parts[0]).$($parts[1]).$($parts[2]).0" }
}
function Read-ToolchainLock([string]$Path) {
    try { $lock = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json }
    catch { Fail "TOOLCHAIN_INTEGRITY_FAILURE" "invalid toolchain lock" }
    $names = @($lock.PSObject.Properties.Name | Sort-Object)
    $expected = @('compiler','schema_version','sha256','tool','url','version')
    if ((Compare-Object $names $expected) -or $lock.schema_version -ne 1 -or $lock.tool -cne 'Inno Setup' -or
        $lock.version -cne '6.7.3' -or $lock.compiler -cne 'ISCC.exe' -or
        $lock.url -cnotmatch '^https://github\.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6\.7\.3\.exe$' -or
        $lock.sha256 -cnotmatch '^[0-9a-f]{64}$') { Fail "TOOLCHAIN_INTEGRITY_FAILURE" "toolchain lock violates the reviewed schema/pin" }
    return $lock
}
function Assert-Under([string]$Child, [string]$Parent, [string]$Kind) {
    $prefix = [IO.Path]::GetFullPath($Parent).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (![IO.Path]::GetFullPath($Child).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { Fail $Kind "path escapes its trusted root" }
}
function Get-InnoCompiler($Lock, [string]$Provided, [string]$TempRoot) {
    if ($Provided) {
        $dir = [IO.Path]::GetFullPath($Provided)
        $compiler = Join-Path $dir $Lock.compiler
        Assert-Under $compiler $dir "TOOLCHAIN_INTEGRITY_FAILURE"
        if (!(Test-Path -LiteralPath $compiler -PathType Leaf)) { Fail "TOOLCHAIN_INTEGRITY_FAILURE" "offline compiler is missing" }
        $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($compiler).ProductVersion
        if ($version -notmatch '^6\.7\.3(?:\D|$)') { Fail "TOOLCHAIN_INTEGRITY_FAILURE" "offline compiler is not locked version 6.7.3" }
        return $compiler
    }
    $distribution = Join-Path $TempRoot 'innosetup-6.7.3.exe'
    try {
        $client = [Net.Http.HttpClient]::new()
        $client.Timeout = [TimeSpan]::FromSeconds(90)
        for ($attempt = 1; $attempt -le 3; $attempt++) {
            try {
                $bytes = $client.GetByteArrayAsync([Uri]$Lock.url).GetAwaiter().GetResult()
                [IO.File]::WriteAllBytes($distribution, $bytes); break
            } catch { if ($attempt -eq 3) { throw }; Start-Sleep -Seconds (2 * $attempt) }
        }
    } catch { Fail "TOOLCHAIN_ACQUISITION_FAILURE" "bounded HTTPS download failed" }
    $actual = (Get-FileHash -LiteralPath $distribution -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -cne $Lock.sha256) { Fail "TOOLCHAIN_INTEGRITY_FAILURE" "Inno distribution SHA-256 mismatch" }
    $install = Join-Path $TempRoot 'inno'
    $args = @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CURRENTUSER',('/DIR=' + $install),'/NOICONS')
    Invoke-Checked $distribution $args "TOOLCHAIN_ACQUISITION_FAILURE"
    return Get-InnoCompiler $Lock $install $TempRoot
}
function Read-Payload([string]$Stage, [string]$Generated) {
    $manifest = Join-Path $Stage 'SYNVEIL-MANIFEST.txt'
    if (!(Test-Path -LiteralPath $manifest -PathType Leaf)) { Fail "PAYLOAD_IDENTITY_FAILURE" "runtime manifest is missing" }
    $entries = @(); $seen = @{}; $inFiles = $false
    foreach ($line in [IO.File]::ReadAllLines($manifest)) {
        if ($line -ceq 'files=sha256 size path') { $inFiles = $true; continue }
        if (!$inFiles) { continue }
        if ($line -notmatch '^([0-9a-f]{64}) ([0-9]+) (.+)$') { Fail "PAYLOAD_IDENTITY_FAILURE" "malformed runtime inventory" }
        $relative = $Matches[3].Replace('\','/')
        if ([IO.Path]::IsPathRooted($relative) -or $relative.Split('/') -contains '..') { Fail "PAYLOAD_IDENTITY_FAILURE" "payload traversal" }
        if ($relative.EndsWith('.exe',[StringComparison]::OrdinalIgnoreCase) -and $relative -notin @('synveil-desktop.exe','synveil-client.exe')) { Fail "PAYLOAD_IDENTITY_FAILURE" "unexpected payload executable" }
        $key = $relative.ToLowerInvariant(); if ($seen.ContainsKey($key)) { Fail "PAYLOAD_IDENTITY_FAILURE" "duplicate/case-colliding payload path" }; $seen[$key] = $true
        $file = [IO.Path]::GetFullPath((Join-Path $Stage $relative)); Assert-Under $file $Stage "PAYLOAD_IDENTITY_FAILURE"
        if (!(Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).LinkType) { Fail "PAYLOAD_IDENTITY_FAILURE" "missing or linked payload file" }
        if ((Get-Item -LiteralPath $file).Length -ne [int64]$Matches[2] -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Matches[1]) { Fail "PAYLOAD_IDENTITY_FAILURE" "payload digest/size mismatch" }
        $entries += [pscustomobject]@{ Path=$relative; Size=[int64]$Matches[2]; Sha256=$Matches[1] }
    }
    foreach ($required in @('synveil-desktop.exe','synveil-client.exe','qt.conf','platforms/qwindows.dll','LICENSE','NOTICE')) { if (!$seen.ContainsKey($required.ToLowerInvariant())) { Fail "PAYLOAD_IDENTITY_FAILURE" "required payload omitted: $required" } }
    $actualFiles = @(Get-ChildItem -LiteralPath $Stage -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($Stage,$_.FullName).Replace('\','/') } | Where-Object { $_ -cne 'SYNVEIL-MANIFEST.txt' })
    if ($actualFiles.Count -ne $entries.Count) { Fail "PAYLOAD_IDENTITY_FAILURE" "unmanifested payload file" }
    $inventory = $entries | Sort-Object Path | ConvertTo-Json -Depth 3
    [IO.File]::WriteAllText((Join-Path $Generated 'payload-inventory.json'), $inventory + "`n", [Text.UTF8Encoding]::new($false))
    $lines = foreach ($entry in ($entries | Sort-Object Path)) {
        $source = (Join-Path $Stage $entry.Path).Replace('"','""'); $dest = [IO.Path]::GetDirectoryName($entry.Path).Replace('\','/')
        if ($dest) { 'Source: "' + $source + '"; DestDir: "{app}/' + $dest + '"; Flags: ignoreversion' } else { 'Source: "' + $source + '"; DestDir: "{app}"; Flags: ignoreversion' }
    }
    # The closed inventory is itself shared by the portable archive and the
    # installed product. It intentionally does not hash itself; every payload
    # byte that it binds is verified before this line is generated.
    $manifestSource = $manifest.Replace('"','""')
    $lines += 'Source: "' + $manifestSource + '"; DestDir: "{app}"; Flags: ignoreversion'
    [IO.File]::WriteAllLines((Join-Path $Generated 'files.iss'), $lines, [Text.UTF8Encoding]::new($false))
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (!(Test-Path (Join-Path $repo 'Cargo.toml')) -or !(Test-Path (Join-Path $repo '.git'))) { Fail "PAYLOAD_IDENTITY_FAILURE" "not a Synveil repository root" }
$revision = (& git -C $repo rev-parse HEAD).Trim(); if ($LASTEXITCODE -ne 0 -or $revision -cnotmatch '^[0-9a-f]{40}$') { Fail "PAYLOAD_IDENTITY_FAILURE" "invalid source revision" }
$sourceEpoch = (& git -C $repo show -s --format=%ct HEAD).Trim(); if ($LASTEXITCODE -ne 0 -or $sourceEpoch -cnotmatch '^[0-9]+$') { Fail "PAYLOAD_IDENTITY_FAILURE" "invalid source timestamp" }; $env:SOURCE_DATE_EPOCH = $sourceEpoch
$output = Full-RepoPath $OutputDirectory $repo; $target = Join-Path $repo 'target'; Assert-Under $output $target "INSTALLER_VERIFY_FAILURE"
$generated = Join-Path $target 'windows-installer-generated'; Remove-Item $generated -Recurse -Force -ErrorAction SilentlyContinue; New-Item $generated,$output -ItemType Directory -Force | Out-Null
$temp = Join-Path ([IO.Path]::GetTempPath()) ('synveil-inno-' + [guid]::NewGuid().ToString('N')); New-Item $temp -ItemType Directory | Out-Null
try {
    $version = Get-WorkspaceVersion (Join-Path $repo 'Cargo.toml')
    if ($RuntimeStagingDirectory) {
        $stage = Full-RepoPath $RuntimeStagingDirectory $repo
    } else {
        $stage = Join-Path $target 'windows-installer-runtime'
        Invoke-Checked 'bash' @((Join-Path $repo 'deploy/packages/build-windows.sh'),('--staging-dir=' + $stage)) "PAYLOAD_BUILD_FAILURE"
    }
    Read-Payload $stage $generated
    $escapedStage = $stage.Replace('"','""'); $escapedOutput = $output.Replace('"','""')
    $defines = @('#define SynveilVersion "' + $version.Product + '"','#define SynveilWindowsVersion "' + $version.Windows + '"','#define SynveilSourceRevision "' + $revision + '"','#define SynveilPayloadDir "' + $escapedStage + '"','#define SynveilOutputDir "' + $escapedOutput + '"')
    [IO.File]::WriteAllLines((Join-Path $generated 'version.iss'), $defines, [Text.UTF8Encoding]::new($false))
    $escapedGenerated = $generated.Replace('"','""')
    $sourceScript = (Join-Path $repo 'deploy/windows/installer/Synveil.iss').Replace('"','""')
    [IO.File]::WriteAllLines((Join-Path $generated 'build.iss'), @('#define GeneratedDir "' + $escapedGenerated + '"','#include "' + $sourceScript + '"'), [Text.UTF8Encoding]::new($false))
    $lock = Read-ToolchainLock (Join-Path $repo 'deploy/windows/installer/toolchain.lock'); $compiler = Get-InnoCompiler $lock $InnoToolchainDirectory $temp
    Remove-Item (Join-Path $output 'SynveilSetup.exe') -Force -ErrorAction SilentlyContinue
    Invoke-Checked $compiler @('/Q',(Join-Path $generated 'build.iss')) "INSTALLER_COMPILE_FAILURE"
    $setup = Join-Path $output 'SynveilSetup.exe'; if (!(Test-Path $setup -PathType Leaf) -or (Get-Item $setup).Length -le 0) { Fail "INSTALLER_VERIFY_FAILURE" "exact output is absent or empty" }
    $bytes = [IO.File]::ReadAllBytes($setup); if ($bytes.Length -lt 512 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { Fail "INSTALLER_VERIFY_FAILURE" "output is not PE" }
    $pe = [BitConverter]::ToInt32($bytes,0x3c); if ($pe -lt 0 -or $pe + 6 -ge $bytes.Length -or [BitConverter]::ToUInt32($bytes,$pe) -ne 0x00004550) { Fail "INSTALLER_VERIFY_FAILURE" "invalid PE header" }
    $machine = [BitConverter]::ToUInt16($bytes,$pe+4); if ($machine -notin @(0x14c,0x8664)) { Fail "INSTALLER_VERIFY_FAILURE" "unexpected Setup PE machine" }
    $metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($setup); if ($metadata.FileVersion -notmatch ('^' + [regex]::Escape($version.Windows) + '(?:\D|$)')) { Fail "INSTALLER_VERIFY_FAILURE" "Setup version metadata mismatch" }
    $ascii = [Text.Encoding]::ASCII.GetString($bytes); $utf16 = [Text.Encoding]::Unicode.GetString($bytes)
    foreach ($marker in @('BEGIN PRIVATE KEY','ghp_','github_pat_')) { if ($ascii.Contains($marker) -or $utf16.Contains($marker)) { Fail "INSTALLER_VERIFY_FAILURE" "secret-like marker in output" } }
    if ($ascii.Contains($repo) -or $utf16.Contains($repo)) { Fail "INSTALLER_VERIFY_FAILURE" "private source path leaked into Setup" }
    Invoke-Checked 'python' @((Join-Path $repo 'scripts/release_manifest.py'),'create','--artifact-root',$output,'--product-version',$version.Product,'--source-commit',$revision,'--output',(Join-Path $output 'SYNVEIL-RELEASE-MANIFEST.json'),'--artifact','{"id":"windows-x86_64-installer","artifact_type":"windows_installer","filename":"SynveilSetup.exe","platform":"windows","architecture":"x86_64","role":"primary_installer","components":["synveil-desktop","synveil-client"]}') "INSTALLER_VERIFY_FAILURE"
    Invoke-Checked 'python' @((Join-Path $repo 'scripts/release_manifest.py'),'validate','--artifact-root',$output,(Join-Path $output 'SYNVEIL-RELEASE-MANIFEST.json')) "INSTALLER_VERIFY_FAILURE"
    $hash=(Get-FileHash $setup -Algorithm SHA256).Hash.ToLowerInvariant(); Write-Host "SynveilSetup.exe size=$((Get-Item $setup).Length) sha256=$hash source=$revision"
} finally { Remove-Item $temp -Recurse -Force -ErrorAction SilentlyContinue }
