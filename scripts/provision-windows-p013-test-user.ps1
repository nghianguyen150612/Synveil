[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidatePattern('^synveil_p013_[0-9a-f]{8}$')][string]$TestUserName,
    [Parameter(Mandatory=$true)][string]$OperatorApproval
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE'
if ($OperatorApproval -cne $approvalText) { throw 'P013_OPERATOR_APPROVAL_REQUIRED' }
if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') {
    throw 'P013_BLOCKED_BY_ENVIRONMENT: test identities may only be provisioned on the isolated native machine'
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'P013_ACCOUNT_PROVISIONING_REQUIRES_SEPARATE_ADMIN_PHASE'
}
if (-not (Get-CimInstance Win32_OperatingSystem).Caption.Contains('Windows 11')) {
    throw 'P013_ACCOUNT_PROVISIONING_UNSUPPORTED_OS'
}
if (Get-LocalUser -Name $TestUserName -ErrorAction SilentlyContinue) {
    throw 'P013_TEST_USER_ALREADY_EXISTS'
}

$securePassword = $null
$created = $false
try {
    $securePassword = Read-Host -AsSecureString 'Enter a one-time password for this disposable account. It is not written to a file or evidence.'
    if ($null -eq $securePassword -or $securePassword.Length -lt 12) { throw 'P013_TEST_PASSWORD_POLICY_FAILURE' }
    New-LocalUser -Name $TestUserName -Password $securePassword -FullName 'Synveil P013 disposable acceptance user' `
        -Description 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT' -AccountNeverExpires | Out-Null
    $created = $true
    $account = Get-LocalUser -Name $TestUserName -ErrorAction Stop
    if (-not $account.Enabled -or $account.Description -cne 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT') {
        throw 'P013_TEST_USER_VERIFICATION_FAILURE'
    }
    $adminGroup = Get-LocalGroup -SID 'S-1-5-32-544' -ErrorAction Stop
    if (@(Get-LocalGroupMember -Group $adminGroup | Where-Object { $_.SID.Value -ceq $account.SID.Value }).Count -gt 0) {
        throw 'P013_TEST_USER_MUST_NOT_BE_ADMINISTRATOR'
    }
    Write-Host 'P013 disposable standard-user account provisioned. Sign in through the Windows sign-in UI for the genuine-logon scenario.'
    Write-Host "Account name for the local operator: $TestUserName"
} catch {
    if ($created) {
        try { Remove-LocalUser -Name $TestUserName -ErrorAction Stop } catch { throw 'P013_ACCOUNT_PROVISIONING_FAILED_CLEANUP_REQUIRED' }
    }
    throw
} finally {
    if ($null -ne $securePassword) { $securePassword.Dispose() }
    $securePassword = $null
}
