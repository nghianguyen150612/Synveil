# Harness synchronization only; never deletes leftover package objects.
function Wait-WindowsPackageRemoval {
    param(
        [Parameter(Mandatory=$true)][string]$PackageRoot,
        [string[]]$PreservedNames = @(),
        [Parameter(Mandatory=$true)][scriptblock]$IsRegistered,
        [ValidateRange(0,120000)][int]$TimeoutMilliseconds = 60000
    )
    $clock = [Diagnostics.Stopwatch]::StartNew()
    do {
        $registered = & $IsRegistered
        $remaining = @()
        if (Test-Path -LiteralPath $PackageRoot) {
            $remaining = @(Get-ChildItem -LiteralPath $PackageRoot -Force -ErrorAction Stop |
                Where-Object { $_.PSIsContainer -or $_.Name -notin $PreservedNames })
        }
        if (!$registered -and $remaining.Count -eq 0) { return }
        if ($clock.ElapsedMilliseconds -ge $TimeoutMilliseconds) {
            $names = ($remaining | Select-Object -First 20 -ExpandProperty Name) -join ', '
            throw "LIFECYCLE_UNINSTALL_TIMEOUT: registration=$registered; package leftovers=[$names]"
        }
        Start-Sleep -Milliseconds 100
    } while ($true)
}

function Remove-WindowsEmptyPackageRoot {
    param([Parameter(Mandatory=$true)][string]$PackageRoot)
    try {
        # Non-recursive removal cannot delete any leftover or adjacent object.
        [IO.Directory]::Delete($PackageRoot, $false)
    } catch [IO.DirectoryNotFoundException] {
        # The uninstaller may have removed the now-empty root first.
    }
}
