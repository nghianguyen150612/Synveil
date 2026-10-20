[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$RuntimeStagingDirectory,
    [string]$OutputDirectory = "target/windows-signed"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Code, [string]$Message) { throw "${Code}: ${Message}" }
function Get-WorkspaceVersion([string]$Path) {
    $text = [IO.File]::ReadAllText($Path)
    $match = [regex]::Match($text, '(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"([^"\r\n]+)"')
    if (!$match.Success) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'Cargo workspace version is missing' }
    return $match.Groups[1].Value
}
function Get-Sha256([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Assert-Under([string]$Path, [string]$Root) {
    $relative = [IO.Path]::GetRelativePath([IO.Path]::GetFullPath($Root), [IO.Path]::GetFullPath($Path))
    if ([IO.Path]::IsPathRooted($relative) -or $relative -eq '..' -or $relative.StartsWith('..' + [IO.Path]::DirectorySeparatorChar) -or $relative.Contains([char]0)) {
        Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime payload path escaped its root'
    }
}
function Invoke-Checked([string]$File, [string[]]$Arguments, [string]$Code) {
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { Fail $Code 'release signing or packaging tool failed' }
}
function Assert-Authenticode([string]$Path, [string]$Thumbprint) {
    Invoke-Checked 'signtool.exe' @('verify','/pa','/all','/tw',$Path) 'WINDOWS_SIGNATURE_FAILURE'
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid -or
        !$signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -cne $Thumbprint) {
        Fail 'WINDOWS_SIGNATURE_FAILURE' 'Authenticode signature or publisher identity is invalid'
    }
}
function Read-Inventory([string]$Stage) {
    $manifest = Join-Path $Stage 'SYNVEIL-MANIFEST.txt'
    if (!(Test-Path -LiteralPath $manifest -PathType Leaf) -or (Get-Item -LiteralPath $manifest).LinkType) {
        Fail 'PAYLOAD_IDENTITY_FAILURE' 'portable runtime inventory is missing or redirected'
    }
    $lines = [IO.File]::ReadAllLines($manifest)
    if ($lines.Count -lt 8 -or $lines[0] -cne 'Synveil Windows portable package manifest' -or
        'format=1' -cnotin $lines -or 'platform=windows-x86_64' -cnotin $lines -or
        'files=sha256 size path' -cnotin $lines) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime inventory header is invalid' }
    $versionLine = @($lines | Where-Object { $_ -cmatch '^version=' })
    $sourceLine = @($lines | Where-Object { $_ -cmatch '^source_revision=' })
    if ($versionLine.Count -ne 1 -or $sourceLine.Count -ne 1) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime identity fields are ambiguous' }
    $version = $versionLine[0].Substring('version='.Length)
    $revision = $sourceLine[0].Substring('source_revision='.Length)
    if ($version -cne $script:ProductVersion -or $revision -cne $script:SourceCommit) {
        Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime version or source commit differs from this checkout'
    }
    $separator = [Array]::IndexOf($lines, 'files=sha256 size path')
    $entries = @{}
    for ($index = $separator + 1; $index -lt $lines.Count; $index++) {
        if ($lines[$index] -notmatch '^([0-9a-f]{64}) ([0-9]+) (.+)$') { Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime inventory record is malformed' }
        $hash = $Matches[1]; $size = [int64]$Matches[2]; $relative = $Matches[3].Replace('\','/')
        if ($relative.StartsWith('/') -or $relative -match '(^|/)\.\.?(/|$)' -or $relative -match '[:<>"|?*\x00-\x1f]') {
            Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime inventory path is unsafe'
        }
        $key = $relative.ToLowerInvariant()
        if ($entries.ContainsKey($key)) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime inventory has duplicate paths' }
        $path = Join-Path $Stage ($relative.Replace('/',[IO.Path]::DirectorySeparatorChar))
        Assert-Under $path $Stage
        if (!(Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).LinkType -or
            (Get-Item -LiteralPath $path).Length -ne $size -or (Get-Sha256 $path) -cne $hash) {
            Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime inventory size or digest mismatch'
        }
        $entries[$key] = [pscustomobject]@{ Path=$relative; Hash=$hash; Size=$size }
    }
    foreach ($required in @('synveil-desktop.exe','synveil-client.exe','qt.conf','platforms/qwindows.dll','LICENSE','NOTICE')) {
        if (!$entries.ContainsKey($required.ToLowerInvariant())) { Fail 'PAYLOAD_IDENTITY_FAILURE' "required runtime file is missing: $required" }
    }
    foreach ($object in Get-ChildItem -LiteralPath $Stage -Force -Recurse) {
        if (($object.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime tree contains a reparse object' }
        if (!$object.PSIsContainer) {
            $relative = [IO.Path]::GetRelativePath($Stage,$object.FullName).Replace('\','/')
            if ($relative -cne 'SYNVEIL-MANIFEST.txt' -and !$entries.ContainsKey($relative.ToLowerInvariant())) {
                Fail 'PAYLOAD_IDENTITY_FAILURE' 'runtime tree contains an unmanifested file'
            }
        }
    }
    return @{ Manifest=$manifest; Lines=$lines; Entries=$entries }
}

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargo = Join-Path $repo 'Cargo.toml'
$script:ProductVersion = Get-WorkspaceVersion $cargo
$script:SourceCommit = (& git -C $repo rev-parse --verify 'HEAD^{commit}').Trim()
if ($LASTEXITCODE -ne 0 -or $script:SourceCommit -notmatch '^[0-9a-f]{40}$') { Fail 'PAYLOAD_IDENTITY_FAILURE' 'checked-out source is not a real Git commit' }
$expectedVersion = $env:SYNVEIL_WINDOWS_SIGNING_EXPECTED_VERSION
if ($expectedVersion -and $expectedVersion -cne $script:ProductVersion) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'requested signing version differs from Cargo workspace version' }

$pfxEncoded = $env:SYNVEIL_WINDOWS_SIGNING_PFX_B64
$pfxPassword = $env:SYNVEIL_WINDOWS_SIGNING_PFX_PASSWORD
$expectedThumbprint = $env:SYNVEIL_WINDOWS_SIGNING_THUMBPRINT
$timestampUrl = $env:SYNVEIL_WINDOWS_TIMESTAMP_URL
if (!$pfxEncoded -or $null -eq $pfxPassword -or !$expectedThumbprint -or !$timestampUrl) {
    Fail 'BLOCKED_SIGNING_INPUT' 'protected Authenticode certificate, timestamp URL, and trusted signer fingerprint are required'
}
if ($pfxEncoded.Length -gt 16MB -or $expectedThumbprint -notmatch '^[0-9A-Fa-f]{40,64}$') {
    Fail 'BLOCKED_SIGNING_INPUT' 'protected Authenticode signing input is malformed'
}
$expectedThumbprint = $expectedThumbprint.ToUpperInvariant()
$timestamp = [Uri]::new($timestampUrl,[UriKind]::Absolute)
if ($timestamp.Scheme -cne 'https' -or $timestamp.UserInfo -or $timestamp.Query -or $timestamp.Fragment -or !$timestamp.IsDefaultPort) {
    Fail 'BLOCKED_SIGNING_INPUT' 'timestamp endpoint must be an approved HTTPS origin'
}
$policyPath = Join-Path $repo 'deploy/release/windows-signing-policy-v1.json'
$policy = Get-Content -LiteralPath $policyPath -Raw | ConvertFrom-Json
if ($policy.schema_version -ne 1 -or $policy.signature_scheme -cne 'Authenticode' -or
    $policy.allowed_timestamp_hosts -notcontains $timestamp.DnsSafeHost.ToLowerInvariant()) {
    Fail 'BLOCKED_SIGNING_INPUT' 'timestamp host is not in the reviewed Windows signing policy'
}

$runtimeInput = [IO.Path]::GetFullPath($RuntimeStagingDirectory)
if (!(Test-Path -LiteralPath $runtimeInput -PathType Container) -or (Get-Item -LiteralPath $runtimeInput).LinkType) {
    Fail 'PAYLOAD_IDENTITY_FAILURE' 'portable runtime input is missing or redirected'
}
$output = [IO.Path]::GetFullPath((Join-Path $repo $OutputDirectory))
Assert-Under $output (Join-Path $repo 'target')
if (Test-Path -LiteralPath $output) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'signed output directory already exists' }
$runtimeRoot = Join-Path $repo 'target/windows-signed-runtime'
if (Test-Path -LiteralPath $runtimeRoot) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'signed runtime staging directory already exists' }

$pfxPath = Join-Path $env:RUNNER_TEMP ('synveil-signing-' + [guid]::NewGuid().ToString('N') + '.pfx')
$thumbCertificate = $null
try {
    try { $pfxBytes = [Convert]::FromBase64String($pfxEncoded) }
    catch { Fail 'BLOCKED_SIGNING_INPUT' 'protected Authenticode certificate is not valid base64' }
    if ($pfxBytes.Length -lt 1024 -or $pfxBytes.Length -gt 12MB) { Fail 'BLOCKED_SIGNING_INPUT' 'protected Authenticode certificate size is invalid' }
    [IO.File]::WriteAllBytes($pfxPath,$pfxBytes)
    $securePassword = ConvertTo-SecureString -String $pfxPassword -AsPlainText -Force
    $imported = Import-PfxCertificate -FilePath $pfxPath -Password $securePassword -CertStoreLocation 'Cert:\CurrentUser\My'
    $thumbCertificate = @($imported | Where-Object { $_.Thumbprint -ceq $expectedThumbprint } | Select-Object -First 1)
    if ($thumbCertificate.Count -ne 1 -or !$thumbCertificate[0].HasPrivateKey -or $thumbCertificate[0].NotAfter -le (Get-Date) -or
        !($thumbCertificate[0].EnhancedKeyUsageList | Where-Object { $_.ObjectId.Value -ceq '1.3.6.1.5.5.7.3.3' })) {
        Fail 'BLOCKED_SIGNING_INPUT' 'certificate is not a valid code-signing identity matching the independently configured thumbprint'
    }
    Copy-Item -LiteralPath $runtimeInput -Destination $runtimeRoot -Recurse
    $inventory = Read-Inventory $runtimeRoot
    foreach ($name in @('synveil-desktop.exe','synveil-client.exe')) {
        $path = Join-Path $runtimeRoot $name
        Invoke-Checked 'signtool.exe' @('sign','/sha1',$expectedThumbprint,'/fd','SHA256','/tr',$timestamp.AbsoluteUri,'/td','SHA256','/d','Synveil',$path) 'WINDOWS_SIGNATURE_FAILURE'
        Assert-Authenticode $path $expectedThumbprint
    }
    $updatedLines = foreach ($line in $inventory.Lines) {
        if ($line -match '^([0-9a-f]{64}) ([0-9]+) (.+)$') {
            $relative = $Matches[3].Replace('\','/')
            $path = Join-Path $runtimeRoot ($relative.Replace('/',[IO.Path]::DirectorySeparatorChar))
            $hash = Get-Sha256 $path
            $size = (Get-Item -LiteralPath $path).Length
            "$hash $size $relative"
        } else { $line }
    }
    [IO.File]::WriteAllText($inventory.Manifest,(($updatedLines -join "`n") + "`n"),[Text.UTF8Encoding]::new($false))
    $inventory = Read-Inventory $runtimeRoot

    $version = $script:ProductVersion
    $zip = Join-Path $output "synveil-$version-windows-x86_64.zip"
    New-Item -ItemType Directory -Path $output | Out-Null
    $epoch = [long](& git -C $repo show -s --format=%ct HEAD)
    if ($LASTEXITCODE -ne 0) { Fail 'PAYLOAD_IDENTITY_FAILURE' 'source commit timestamp is unavailable' }
    Invoke-Checked 'python' @(
        (Join-Path $repo 'scripts/create-reproducible-zip.py'), '--root',$runtimeRoot,'--output',$zip,
        '--source-date-epoch',[string]$epoch,'--executable','synveil-desktop.exe','--executable','synveil-client.exe',
        '--required-file','synveil-desktop.exe','--required-file','synveil-client.exe','--required-file','qt.conf',
        '--required-file','platforms/qwindows.dll','--required-file','LICENSE','--required-file','NOTICE',
        '--required-file','SYNVEIL-MANIFEST.txt') 'WINDOWS_PACKAGE_FAILURE'

    Invoke-Checked 'powershell.exe' @('-NoProfile','-File',(Join-Path $repo 'scripts/build-windows-installer.ps1'),
        '-RuntimeStagingDirectory',$runtimeRoot,'-OutputDirectory',$output) 'WINDOWS_PACKAGE_FAILURE'
    $setup = Join-Path $output 'SynveilSetup.exe'
    Invoke-Checked 'signtool.exe' @('sign','/sha1',$expectedThumbprint,'/fd','SHA256','/tr',$timestamp.AbsoluteUri,'/td','SHA256','/d','Synveil Setup',$setup) 'WINDOWS_SIGNATURE_FAILURE'
    Assert-Authenticode $setup $expectedThumbprint

    $manifest = Join-Path $output 'SYNVEIL-RELEASE-MANIFEST.json'
    Remove-Item -LiteralPath $manifest -Force -ErrorAction SilentlyContinue
    $portable = '{"id":"windows-x86_64-portable","artifact_type":"windows_portable_zip","filename":"' + [IO.Path]::GetFileName($zip) + '","platform":"windows","architecture":"x86_64","role":"portable","components":["synveil-desktop","synveil-client"]}'
    $installer = '{"id":"windows-x86_64-installer","artifact_type":"windows_installer","filename":"SynveilSetup.exe","platform":"windows","architecture":"x86_64","role":"primary_installer","components":["synveil-desktop","synveil-client"]}'
    Invoke-Checked 'python' @((Join-Path $repo 'scripts/release_manifest.py'),'create','--artifact-root',$output,
        '--product-version',$version,'--source-commit',$script:SourceCommit,'--output',$manifest,'--artifact',$portable,'--artifact',$installer) 'WINDOWS_PACKAGE_FAILURE'
    Invoke-Checked 'python' @((Join-Path $repo 'scripts/release_manifest.py'),'validate','--artifact-root',$output,'--source-commit',$script:SourceCommit,$manifest) 'WINDOWS_PACKAGE_FAILURE'

    $files = @('synveil-desktop.exe','synveil-client.exe','SynveilSetup.exe') | ForEach-Object {
        $path = if ($_ -eq 'SynveilSetup.exe') { $setup } else { Join-Path $runtimeRoot $_ }
        $signature = Get-AuthenticodeSignature -LiteralPath $path
        [pscustomobject]@{ filename=$_; size_bytes=(Get-Item -LiteralPath $path).Length; sha256=(Get-Sha256 $path); signature_status=$signature.Status.ToString(); signer_thumbprint=$signature.SignerCertificate.Thumbprint }
    }
    $evidence = [ordered]@{ schema_version=1; status='VERIFIED_SIGNED_CANDIDATE'; signature_scheme='Authenticode'; signer_thumbprint=$expectedThumbprint; timestamp_host=$timestamp.DnsSafeHost.ToLowerInvariant(); source_commit=$script:SourceCommit; product_version=$version; files=$files }
    [IO.File]::WriteAllText((Join-Path $output 'windows-signing-evidence.json'),($evidence | ConvertTo-Json -Depth 5) + "`n",[Text.UTF8Encoding]::new($false))
    Get-FileHash -LiteralPath $zip,$setup -Algorithm SHA256 | ForEach-Object { Write-Output ($_.Path + ' ' + $_.Hash.ToLowerInvariant()) }
} finally {
    Remove-Item -LiteralPath $pfxPath -Force -ErrorAction SilentlyContinue
    if ($thumbCertificate -and $thumbCertificate.Count -eq 1) {
        Remove-Item -LiteralPath ("Cert:\CurrentUser\My\" + $expectedThumbprint) -Force -ErrorAction SilentlyContinue
    }
}
