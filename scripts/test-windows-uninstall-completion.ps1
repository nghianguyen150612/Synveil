# Filesystem/registration fixtures, not native installer acceptance evidence.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-uninstall-completion.ps1')
$root = Join-Path ([IO.Path]::GetTempPath()) ('synveil-uninstall-wait-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
function Assert-Rejected([scriptblock]$Probe) {
    try { & $Probe } catch {
        if ($_.Exception.Message -like 'LIFECYCLE_UNINSTALL_TIMEOUT:*') { return }
        throw
    }
    throw 'UNINSTALL_WAIT_FIXTURE_FAILURE: leftover/registration was accepted'
}
try {
    $owned = Join-Path $root 'owned.dll'
    Set-Content -LiteralPath $owned -Value 'owned payload'
    Assert-Rejected { Wait-WindowsPackageRemoval -PackageRoot $root -IsRegistered { $false } -TimeoutMilliseconds 0 }
    if (!(Test-Path -LiteralPath $owned)) { throw 'UNINSTALL_WAIT_FIXTURE_FAILURE: helper deleted a leftover' }
    try {
        Remove-WindowsEmptyPackageRoot -PackageRoot $root
        throw 'UNINSTALL_WAIT_FIXTURE_FAILURE: nonempty root was accepted'
    } catch [IO.IOException] {
        if (!(Test-Path -LiteralPath $owned)) { throw 'UNINSTALL_WAIT_FIXTURE_FAILURE: root cleanup deleted a leftover' }
    }
    Remove-Item -LiteralPath $owned
    $directory = Join-Path $root 'platforms'
    New-Item -ItemType Directory -Path $directory | Out-Null
    Assert-Rejected { Wait-WindowsPackageRemoval -PackageRoot $root -IsRegistered { $false } -TimeoutMilliseconds 0 }
    Remove-Item -LiteralPath $directory
    Assert-Rejected { Wait-WindowsPackageRemoval -PackageRoot $root -IsRegistered { $true } -TimeoutMilliseconds 0 }
    $note = Join-Path $root 'user-note.txt'
    Set-Content -LiteralPath $note -Value 'unknown adjacent user data'
    $before = (Get-FileHash -LiteralPath $note).Hash
    Assert-Rejected { Wait-WindowsPackageRemoval -PackageRoot $root -IsRegistered { $false } -TimeoutMilliseconds 0 }
    Wait-WindowsPackageRemoval -PackageRoot $root -PreservedNames @('user-note.txt') -IsRegistered { $false } -TimeoutMilliseconds 0
    if ((Get-FileHash -LiteralPath $note).Hash -cne $before) { throw 'UNINSTALL_WAIT_FIXTURE_FAILURE: preserved data changed' }
    Remove-Item -LiteralPath $note
    Remove-WindowsEmptyPackageRoot -PackageRoot $root
    Remove-WindowsEmptyPackageRoot -PackageRoot $root
    if (Test-Path -LiteralPath $root) { throw 'UNINSTALL_WAIT_FIXTURE_FAILURE: empty root remains' }
    Write-Output 'Uninstall completion fixtures: PASS (not native acceptance)'
} finally {
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
