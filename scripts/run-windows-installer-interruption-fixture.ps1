$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
# This fixture's installer handoff executes the real release synveil-client.exe.
$client = Join-Path $repo 'target\release\synveil-client.exe'
if (!(Test-Path -LiteralPath $client -PathType Leaf)) {
    throw 'P044_WINDOWS_FIXTURE_FAILURE: release synveil-client.exe was not built.'
}

$target = Join-Path $repo 'target'
$oldStage = Join-Path $target 'p044-windows-runtime-old'
$newStage = Join-Path $target 'p044-windows-runtime-new'
$oldOutput = Join-Path $target 'p044-windows-setup-old'
$newOutput = Join-Path $target 'p044-windows-setup-new'
$evidenceDirectory = Join-Path $target 'p044-windows-evidence'
$evidencePath = Join-Path $evidenceDirectory 'inno-process-interruption.json'

function Write-RandomFixtureFile([string]$Path, [int]$SizeMiB) {
    $directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $buffer = [byte[]]::new(1MB)
    try {
        for ($index = 0; $index -lt $SizeMiB; $index++) {
            [System.Security.Cryptography.RandomNumberGenerator]::Fill($buffer)
            $stream.Write($buffer, 0, $buffer.Length)
        }
        $stream.Flush($true)
    } finally { $stream.Dispose() }
}

function Write-FixtureManifest([string]$Stage, [string]$Version) {
    $entries = [Collections.Generic.List[string]]::new()
    $files = @(Get-ChildItem -LiteralPath $Stage -File -Recurse |
        Where-Object { $_.Name -cne 'SYNVEIL-MANIFEST.txt' } |
        Sort-Object { [IO.Path]::GetRelativePath($Stage, $_.FullName).Replace('\','/') })
    foreach ($file in $files) {
        $relative = [IO.Path]::GetRelativePath($Stage, $file.FullName).Replace('\','/')
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $entries.Add("$hash $($file.Length) $relative")
    }
    $lines = @(
        'Synveil Windows portable package manifest',
        'format=1',
        "version=$Version",
        'platform=windows-x86_64',
        'files=sha256 size path'
    ) + @($entries)
    [IO.File]::WriteAllLines((Join-Path $Stage 'SYNVEIL-MANIFEST.txt'), $lines, [Text.UTF8Encoding]::new($false))
}

function New-FixtureStage([string]$Path, [string]$Version, [string]$LicenseText, [bool]$Obsolete) {
    Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path (Join-Path $Path 'platforms') -Force | Out-Null
    Copy-Item -LiteralPath $client -Destination (Join-Path $Path 'synveil-client.exe')
    # Setup requires a desktop executable path and icon target. This synthetic
    # fixture never launches it; the real client remains responsible for the
    # installer startup-preference handoff.
    Copy-Item -LiteralPath $client -Destination (Join-Path $Path 'synveil-desktop.exe')
    [IO.File]::WriteAllText((Join-Path $Path 'qt.conf'), "[Paths]`nPlugins=plugins`n", [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $Path 'platforms\qwindows.dll'), 'P044 synthetic Qt platform placeholder', [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $Path 'LICENSE'), $LicenseText, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $Path 'NOTICE'), 'P044 synthetic notices payload', [Text.UTF8Encoding]::new($false))
    Write-RandomFixtureFile (Join-Path $Path 'runtime-payload.bin') 16
    if ($Obsolete) {
        [IO.File]::WriteAllText((Join-Path $Path 'p044-obsolete-owned.txt'), 'P044 obsolete owned fixture', [Text.UTF8Encoding]::new($false))
    }
    Write-FixtureManifest $Path $Version
}

New-FixtureStage $oldStage '1.0.0' 'P044 older synthetic package license' $true
New-FixtureStage $newStage '1.1.0' 'P044 newer synthetic package license' $false
$newerLicenseHash = (Get-FileHash -LiteralPath (Join-Path $newStage 'LICENSE') -Algorithm SHA256).Hash.ToLowerInvariant()
$newerRuntimePayloadHash = (Get-FileHash -LiteralPath (Join-Path $newStage 'runtime-payload.bin') -Algorithm SHA256).Hash.ToLowerInvariant()

& (Join-Path $PSScriptRoot 'build-windows-installer.ps1') `
    -RuntimeStagingDirectory $oldStage `
    -OutputDirectory $oldOutput `
    -LifecycleFixtureVersion '1.0.0'
& (Join-Path $PSScriptRoot 'build-windows-installer.ps1') `
    -RuntimeStagingDirectory $newStage `
    -OutputDirectory $newOutput `
    -LifecycleFixtureVersion '1.1.0'

& (Join-Path $PSScriptRoot 'test-windows-installer-interruption.ps1') `
    -OlderFixtureSetup (Join-Path $oldOutput 'SynveilSetup.exe') `
    -NewerFixtureSetup (Join-Path $newOutput 'SynveilSetup.exe') `
    -NewerLicenseHash $newerLicenseHash `
    -NewerRuntimePayloadHash $newerRuntimePayloadHash `
    -EvidencePath $evidencePath
