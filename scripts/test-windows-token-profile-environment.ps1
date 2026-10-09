# Environment regression fixture, not native installation acceptance.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-known-folders.ps1')
$profileRoot = [SynveilKnownFolders]::CurrentUserProfile()
$names = @('USERPROFILE','LOCALAPPDATA','APPDATA','HOMEDRIVE','HOMEPATH','TEMP','TMP','USERNAME','USERDOMAIN')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    # Model the environment inherited from a different launcher account.
    $env:USERPROFILE = 'C:\unavailable-launcher-profile'
    $env:LOCALAPPDATA = 'C:\unavailable-launcher-profile\AppData\Local'
    $env:TEMP = 'C:\unavailable-launcher-profile\Temp'
    $env:TMP = $env:TEMP
    Set-WindowsTokenProfileEnvironment
    $local = Join-Path $profileRoot 'AppData\Local'
    $expected = @{
        USERPROFILE=$profileRoot; LOCALAPPDATA=$local
        APPDATA=(Join-Path $profileRoot 'AppData\Roaming')
        TEMP=(Join-Path $local 'Temp'); TMP=(Join-Path $local 'Temp')
    }
    foreach ($name in $expected.Keys) {
        if (![string]::Equals([Environment]::GetEnvironmentVariable($name, 'Process'), $expected[$name], [StringComparison]::OrdinalIgnoreCase)) {
            throw "TOKEN_PROFILE_REGRESSION: $name retained the launcher namespace"
        }
    }
    $probe = Join-Path $env:TEMP ('synveil-profile-probe-' + [Guid]::NewGuid().ToString('N'))
    try {
        [IO.File]::WriteAllText($probe, 'non-secret environment fixture')
        if ([IO.File]::ReadAllText($probe) -cne 'non-secret environment fixture') { throw 'TOKEN_PROFILE_REGRESSION: child temporary file unreadable' }
    } finally { if (Test-Path -LiteralPath $probe) { Remove-Item -LiteralPath $probe } }
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
Write-Host 'Token profile environment regression fixture: PASS (not native acceptance)'
