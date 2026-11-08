# Test-only PostgreSQL fixture from the GitHub-managed Windows 2025 image.
# Never installs a service, downloads binaries, or opens the image's PGDATA.
[CmdletBinding()]
param([ValidateSet('start','restart','stop','verify')][string]$Action = 'verify')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-security.ps1')
$P027PgVersion = '17.11'
$P027Tools = @('initdb.exe','pg_ctl.exe','pg_isready.exe','postgres.exe','psql.exe')
$P027ImageReferences = @{
    'win25' = 'https://github.com/actions/runner-images/blob/fad015e47059f40cf0786dcb4593384f6a0720d3/images/windows/Windows2025-Readme.md'
    'win25-vs2026' = 'https://github.com/actions/runner-images/blob/win25-vs2026/20261103.298/images/windows/Windows2025-VS2026-Readme.md'
}

function Assert-P027Image {
    if ($env:GITHUB_ACTIONS -cne 'true') { throw 'P027_PG_PROVENANCE_FAILURE: GitHub Actions required' }
    if ($env:RUNNER_OS -cne 'Windows') { throw 'P027_PG_PROVENANCE_FAILURE: Windows host required' }
    if ($env:ImageOS -cnotin @('win25','win25-vs2026')) { throw 'P027_PG_PROVENANCE_FAILURE: unreviewed Windows image variant' }
    if ($env:ImageVersion -cnotmatch '^\d{8}\.\d+\.\d+$') { throw 'P027_PG_PROVENANCE_FAILURE: image version required' }
    if ($env:SYNVEIL_P027_DISPOSABLE -cne '1') { throw 'P027_PG_PROVENANCE_FAILURE: disposable opt-in required' }
    if (![IO.Path]::IsPathFullyQualified($env:RUNNER_TEMP)) { throw 'P027_PG_PROVENANCE_FAILURE: runner temp missing' }
    Assert-NoReparseAncestry $env:RUNNER_TEMP
}
function Get-P027Inventory {
    Assert-P027Image
    $bin = Join-Path $env:ProgramFiles 'PostgreSQL\17\bin'
    foreach ($name in $P027Tools) {
        $path = Join-Path $bin $name
        Assert-NoReparseAncestry $path $true
        $item = Get-Item -LiteralPath $path
        if ($item.Length -le 0) { throw 'P027_PG_INTEGRITY_FAILURE: empty runner tool' }
        [pscustomobject]@{ name=$name; path=$path; size=$item.Length; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
}
function Assert-P027Inventory($Expected, $Actual) {
    if (@($Expected).Count -ne $P027Tools.Count -or @($Actual).Count -ne $P027Tools.Count) {
        throw 'P027_PG_INTEGRITY_FAILURE: incomplete tool inventory'
    }
    foreach ($name in $P027Tools) {
        $before = @($Expected | Where-Object name -CEQ $name)
        $current = @($Actual | Where-Object name -CEQ $name)
        if ($before.Count -ne 1 -or $current.Count -ne 1 -or
            $before[0].path -cne $current[0].path -or $before[0].size -ne $current[0].size -or
            $before[0].sha256 -cnotmatch '^[0-9a-f]{64}$' -or $before[0].sha256 -cne $current[0].sha256) {
            throw 'P027_PG_INTEGRITY_FAILURE: runner tools changed; no tool executed'
        }
    }
}
function Assert-P027Version([string]$Text) {
    if ($Text.Trim() -cne "postgres (PostgreSQL) $P027PgVersion") {
        throw 'P027_PG_VERSION_FAILURE: reviewed PostgreSQL 17.11 required'
    }
}
function Assert-P027State($State) {
    Assert-P027Image
    if ($State.marker -cne 'Synveil-P027-disposable-v1' -or $State.version -cne $P027PgVersion -or
        $State.image_os -cne $env:ImageOS -or $State.image_version -cne $env:ImageVersion -or $State.port -ne 55427) {
        throw 'P027_PG_PROVENANCE_FAILURE: foreign fixture state'
    }
    $root = [IO.Path]::GetFullPath($State.root)
    if ([IO.Path]::GetDirectoryName($root) -cne [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') -or
        [IO.Path]::GetFileName($root) -cnotmatch '^p027-pg-[a-f0-9]{32}$' -or
        $State.data -cne (Join-Path $root 'data')) { throw 'P027_PG_SCOPE_FAILURE: foreign data directory' }
    Assert-NoReparseAncestry $root
    Assert-NoReparseAncestry $State.data
    Assert-P027Inventory $State.tools @(Get-P027Inventory)
}
function Invoke-P027Tool($State, [string]$Name, [string[]]$Arguments) {
    Assert-P027State $State
    if ($Name -cnotin $P027Tools) { throw 'P027_PG_INTEGRITY_FAILURE: unknown tool' }
    $path = ($State.tools | Where-Object name -CEQ $Name).path
    $output = & $path @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "P027_PG_TOOL_FAILURE: $Name exit $LASTEXITCODE" }
    return $output
}
function Get-P027StatePath { return Join-Path $env:RUNNER_TEMP 'p027-postgres-state.json' }
function Read-P027State {
    $path = Get-P027StatePath
    Assert-NoReparseAncestry $path $true
    $state = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    Assert-P027State $state
    return $state
}
function Write-P027Evidence($State) {
    $directory = Join-Path (Get-Location) 'target/p027-evidence'
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $evidence = [ordered]@{
        source_head=(& git rev-parse HEAD); image_os=$State.image_os; image_version=$State.image_version
        reviewed_image_reference=$P027ImageReferences[$State.image_os]; postgres_version=$State.version; tools=$State.tools
        fixture_scope='new UUID directory under RUNNER_TEMP; loopback only; no installed service/data access'
    }
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $directory 'windows-postgres-provenance.json') -Encoding utf8
}
function Start-P027Postgres {
    Assert-P027Image
    $statePath = Get-P027StatePath
    if (Test-Path -LiteralPath $statePath) { throw 'P027_PG_SCOPE_FAILURE: existing fixture state preserved' }
    $tools = @(Get-P027Inventory)
    $postgres = ($tools | Where-Object name -CEQ 'postgres.exe').path
    $version = & $postgres --version
    if ($LASTEXITCODE -ne 0) { throw 'P027_PG_VERSION_FAILURE: runner identity query failed' }
    Assert-P027Version $version
    # Refuse an occupied port instead of authenticating to somebody else's DB.
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,55427)
    try { $listener.Start() } finally { $listener.Stop() }
    $root = Join-Path $env:RUNNER_TEMP ('p027-pg-' + [guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($root) | Out-Null
    $data = Join-Path $root 'data'
    [IO.Directory]::CreateDirectory($data) | Out-Null
    $state = [pscustomobject]@{ marker='Synveil-P027-disposable-v1'; version=$P027PgVersion; image_os=$env:ImageOS; image_version=$env:ImageVersion; port=55427; root=$root; data=$data; tools=$tools }
    $json = [Text.Encoding]::UTF8.GetBytes(($state | ConvertTo-Json -Depth 8))
    $stream = [IO.File]::Open($statePath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try { $stream.Write($json,0,$json.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    # Disposable fixture password, no real credential or bearer material.
    $passwordFile = Join-Path $root 'fixture-password.txt'
    [IO.File]::WriteAllText($passwordFile,'p027-disposable',[Text.UTF8Encoding]::new($false))
    try {
        Invoke-P027Tool $state 'initdb.exe' @('-D',$data,'-U','p027','--auth=scram-sha-256','--encoding=UTF8','--locale=C',"--pwfile=$passwordFile") | Out-Host
    } finally { Remove-Item -LiteralPath $passwordFile -Force }
    Add-Content -LiteralPath (Join-Path $data 'postgresql.conf') -Value "`nlisten_addresses = '127.0.0.1'`nport = 55427`n"
    Invoke-P027Tool $state 'pg_ctl.exe' @('start','-D',$data,'-l',(Join-Path $root 'postgres.log'),'-w','-t','60') | Out-Host
    $env:PGPASSWORD = 'p027-disposable'
    try {
        Invoke-P027Tool $state 'psql.exe' @('-h','127.0.0.1','-p','55427','-U','p027','-d','postgres','-v','ON_ERROR_STOP=1','-c','CREATE DATABASE p027') | Out-Host
        $server = Invoke-P027Tool $state 'psql.exe' @('-h','127.0.0.1','-p','55427','-U','p027','-d','p027','-Atc','SHOW server_version_num')
        if ($server.Trim() -cne '170011') { throw 'P027_PG_VERSION_FAILURE: wrong server' }
    } finally { Remove-Item Env:PGPASSWORD }
    Write-P027Evidence $state
    'SYNVEIL_P027_DATABASE_URL=postgresql://p027:p027-disposable@127.0.0.1:55427/p027' | Out-File -LiteralPath $env:GITHUB_ENV -Append -Encoding utf8
}
function Restart-P027Postgres {
    $state = Read-P027State
    Invoke-P027Tool $state 'pg_ctl.exe' @('restart','-D',$state.data,'-m','fast','-w','-t','60','-l',(Join-Path $state.root 'postgres.log')) | Out-Host
}
function Stop-P027Postgres {
    Assert-P027Image
    if (!(Test-Path -LiteralPath (Get-P027StatePath))) { Write-Host 'P027 fixture was not provisioned'; return }
    $state = Read-P027State
    $pidFile = Join-Path $state.data 'postmaster.pid'
    if (Test-Path -LiteralPath $pidFile) {
        Invoke-P027Tool $state 'pg_ctl.exe' @('stop','-D',$state.data,'-m','fast','-w','-t','60') | Out-Host
    }
    $log = Join-Path $state.root 'postgres.log'
    if (Test-Path -LiteralPath $log -PathType Leaf) {
        Copy-Item -LiteralPath $log -Destination 'target/p027-evidence/windows-postgres-server.log'
    }
    # Preserve logs/data for diagnostics. Never remove or stop an installed service.
}
if ($MyInvocation.InvocationName -ne '.') {
    switch ($Action) {
        'start' { Start-P027Postgres }
        'restart' { Restart-P027Postgres }
        'stop' { Stop-P027Postgres }
        'verify' { Assert-P027Image; $null = @(Get-P027Inventory); Assert-P027Version (& (Join-Path $env:ProgramFiles 'PostgreSQL\17\bin\postgres.exe') --version); Get-P027Inventory | Format-Table }
    }
}
