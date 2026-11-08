# Negative guard tests; none starts a server or executes a supplied artifact.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'p027-windows-postgres.ps1')
function Reject([scriptblock]$Probe, [string]$Expected) {
    try { & $Probe } catch {
        if ($_.Exception.Message -like "*$Expected*") { return }
        throw
    }
    throw "P027 guard unexpectedly accepted: $Expected"
}
Assert-P027Image
$inventory = @(Get-P027Inventory)
Assert-P027Inventory $inventory $inventory
$bad = $inventory | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$bad[0].sha256 = '0' * 64
Reject { Assert-P027Inventory $bad $inventory } 'P027_PG_INTEGRITY_FAILURE'
$bad = $inventory | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$bad[0].path = Join-Path $env:RUNNER_TEMP 'untrusted.exe'
Reject { Assert-P027Inventory $bad $inventory } 'P027_PG_INTEGRITY_FAILURE'
Reject { Assert-P027Inventory @() $inventory } 'P027_PG_INTEGRITY_FAILURE'
Reject { Assert-P027Version 'postgres (PostgreSQL) 17.10' } 'P027_PG_VERSION_FAILURE'
Reject { Assert-P027Version 'postgres (PostgreSQL) 18.1' } 'P027_PG_VERSION_FAILURE'
$validRoot = Join-Path $env:RUNNER_TEMP ('p027-pg-' + [guid]::NewGuid().ToString('N'))
$state = [pscustomobject]@{ marker='Synveil-P027-disposable-v1'; version='17.11'; image_os=$env:ImageOS; image_version=$env:ImageVersion; port=55427; root=$validRoot; data=(Join-Path $validRoot 'data'); tools=$inventory }
$invalid = $state | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$invalid.tools[0].sha256 = '0' * 64
Reject { Invoke-P027Tool $invalid 'pg_ctl.exe' @('start') } 'P027_PG_INTEGRITY_FAILURE'
if (Test-Path -LiteralPath $validRoot) { throw 'P027 negative guard promoted invalid fixture state' }
$state.data = $env:PGDATA
Reject { Assert-P027State $state } 'P027_PG_SCOPE_FAILURE'
$state.data = Join-Path $validRoot 'data'; $state.marker = 'untrusted-cache'
Reject { Assert-P027State $state } 'P027_PG_PROVENANCE_FAILURE'
$original = $env:ImageOS
try { $env:ImageOS='unreviewed-image'; Reject { Assert-P027Image } 'P027_PG_PROVENANCE_FAILURE' } finally { $env:ImageOS=$original }
$original = $env:ImageVersion
try { $env:ImageVersion=''; Reject { Assert-P027Image } 'P027_PG_PROVENANCE_FAILURE' } finally { $env:ImageVersion=$original }
$original = $env:SYNVEIL_P027_DISPOSABLE
try { $env:SYNVEIL_P027_DISPOSABLE='0'; Reject { Assert-P027Image } 'P027_PG_PROVENANCE_FAILURE' } finally { $env:SYNVEIL_P027_DISPOSABLE=$original }
$powershell = (Get-Process -Id $PID).Path
Reject { Invoke-P027BoundedProcess $powershell @('-NoProfile','-Command','exit 7') } 'P027_PG_TOOL_FAILURE'
Reject { Invoke-P027BoundedProcess $powershell @('-NoProfile','-Command','Start-Sleep -Seconds 60') $false 300 } 'P027_PG_PROCESS_TIMEOUT'
Write-Host 'P027 Windows PostgreSQL provenance/version/hash/scope/process guard negatives: 13 passed; no server started' 
