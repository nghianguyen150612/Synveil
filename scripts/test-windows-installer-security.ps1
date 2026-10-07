# Source/guard fixtures only. This is not clean-machine/runtime qualification.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-security.ps1')
function Expect-Rejected([scriptblock]$Action) {
    $rejected=$false
    try { & $Action } catch { $rejected=$true }
    if (!$rejected) { throw 'P043_SECURITY_FAILURE: unsafe input accepted' }
}
foreach ($value in @('../evil','C:\absolute','C:relative','\rooted','x//y','x/./y','NUL.dll','x/COM1.dll','x/a:stream','x/trailing.','x/{app}',"x/`nsecret")) {
    Expect-Rejected { Assert-SafeRelativeIdentity $value }
}
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$fixture = Join-Path $repo ('target/windows-security-' + [guid]::NewGuid().ToString('N'))
$stage=Join-Path $fixture 'payload'; New-Item -ItemType Directory -Path $stage -Force | Out-Null
try {
    $files=@('synveil-desktop.exe','synveil-client.exe','qt.conf','platforms/qwindows.dll','LICENSE','NOTICE')
    $lines=@('Synveil Windows portable package manifest','format=1','version=1.0.0','platform=windows-x86_64','files=sha256 size path')
    foreach ($name in $files) {
        $path=Join-Path $stage $name; New-Item -ItemType Directory -Path (Split-Path $path) -Force | Out-Null
        # Deliberately inert synthetic bytes; no executable is launched.
        [IO.File]::WriteAllBytes($path,[Text.Encoding]::UTF8.GetBytes('synthetic P043 non-executable fixture'))
        $lines += ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() + ' ' + (Get-Item -LiteralPath $path).Length + ' ' + $name)
    }
    [IO.File]::WriteAllLines((Join-Path $stage 'SYNVEIL-MANIFEST.txt'),$lines,[Text.UTF8Encoding]::new($false))
    $output=Join-Path $fixture 'output'
    & (Join-Path $PSScriptRoot 'build-windows-installer.ps1') -RuntimeStagingDirectory $stage -OutputDirectory $output -LifecycleFixtureVersion '1.0.0'
    $setup=Join-Path $output 'SynveilSetup.exe'
    $ownedRoot=Join-Path $env:LOCALAPPDATA 'Programs\Synveil'
    if (Test-Path -LiteralPath $ownedRoot) { throw 'P043_FIXTURE_FAILURE: host already has an install root' }
    $sentinel=Join-Path $fixture 'sentinel'; New-Item $sentinel -ItemType Directory | Out-Null
    Set-Content (Join-Path $sentinel 'keep.txt') 'unknown adjacent sentinel'
    foreach ($extra in @('/DIR=C:\escaped','/REPAIR=0','/STARTUP=0 /STARTUP=1','/LOADINF=untrusted.inf')) {
        $process=Start-Process -FilePath $setup -ArgumentList ('/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LAUNCH=0 '+$extra) -PassThru
        if (!$process.WaitForExit(30000)) { $process.Kill(); throw 'P043_FIXTURE_TIMEOUT' }
        if ($process.ExitCode -eq 0 -or (Test-Path -LiteralPath $ownedRoot)) { throw 'P043_SECURITY_FAILURE: override reached mutation' }
    }
    # Trusted prior inventory must authorize every existing copy destination,
    # including Repair. A new payload filename cannot claim an adjacent file.
    $key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1'
    if (Test-Path $key) { throw 'P043_FIXTURE_FAILURE: host has an existing registration' }
    New-Item -ItemType Directory -Path $ownedRoot -Force | Out-Null
    try {
        $previous=@($lines | Where-Object { $_ -notmatch ' qt\.conf$' })
        $manifest=Join-Path $ownedRoot 'SYNVEIL-MANIFEST.txt'
        [IO.File]::WriteAllLines($manifest,$previous,[Text.UTF8Encoding]::new($false))
        Set-Content (Join-Path $ownedRoot 'qt.conf') 'unknown adjacent file'
        New-Item $key -Force | Out-Null
        New-ItemProperty $key -Name DisplayVersion -Value '1.0.0' -PropertyType String | Out-Null
        New-ItemProperty $key -Name InstallLocation -Value $ownedRoot -PropertyType String | Out-Null
        New-ItemProperty $key -Name SynveilManifestSha256 -Value (Get-FileHash $manifest -Algorithm SHA256).Hash.ToLowerInvariant() -PropertyType String | Out-Null
        $process=Start-Process -FilePath $setup -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /REPAIR=1 /STARTUP=0 /LAUNCH=0' -PassThru
        if (!$process.WaitForExit(30000)) { $process.Kill(); throw 'P043_FIXTURE_TIMEOUT' }
        if ($process.ExitCode -eq 0 -or (Get-Content (Join-Path $ownedRoot 'qt.conf') -Raw).Trim() -cne 'unknown adjacent file') {
            throw 'P043_SECURITY_FAILURE: repair claimed an unowned destination'
        }
        if (@(Get-ChildItem -LiteralPath $ownedRoot).Count -ne 2) { throw 'P043_SECURITY_FAILURE: repair reached payload mutation' }
    } finally {
        Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $ownedRoot -Recurse -Force
    }
    New-Item (Split-Path $ownedRoot) -ItemType Directory -Force | Out-Null
    New-Item -ItemType Junction -Path $ownedRoot -Target $sentinel | Out-Null
    try {
        Expect-Rejected { Assert-NoReparseAncestry (Join-Path $ownedRoot 'nested\payload.dll') $true }
        Expect-Rejected { Get-OwnedRegisteredUninstaller '"C:\untrusted\unins123.exe" /SILENT' $ownedRoot }
        $process=Start-Process -FilePath $setup -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /STARTUP=0 /LAUNCH=0' -PassThru
        if (!$process.WaitForExit(30000)) { $process.Kill(); throw 'P043_FIXTURE_TIMEOUT' }
        if ($process.ExitCode -eq 0) { throw 'P043_SECURITY_FAILURE: junction install accepted' }
        if (@(Get-ChildItem -LiteralPath $sentinel).Count -ne 1) { throw 'P043_SECURITY_FAILURE: junction target was modified' }
    } finally { [IO.Directory]::Delete($ownedRoot) } # delete junction object, never its target
    Write-Host 'P043 Windows compiler/options/reparse fixtures: PASS'
} finally { Remove-Item -LiteralPath $fixture -Recurse -Force }
