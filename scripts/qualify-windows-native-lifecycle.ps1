[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ApprovedMachineUuidSha256,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$CandidatePath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$CandidateSha256,
    [Parameter(Mandatory=$true)][string]$ManifestPath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [Parameter(Mandatory=$true)][PSCredential]$ObserverCredential,
    [Parameter(Mandatory=$true)][PSCredential]$SubjectCredential
)
$ErrorActionPreference = 'Stop'
# Operator approval must come from an external machine allocation/review. A local
# marker alone is insufficient. This entrypoint never provisions accounts/logs off.
if ($OperatorApproval -cne 'I_APPROVE_P028_DISPOSABLE_MACHINE_ROLE_QUALIFICATION') {
    throw 'P028_EXPLICIT_MACHINE_APPROVAL_REQUIRED'
}
if (-not $IsWindows -or $env:PROCESSOR_ARCHITECTURE -cne 'AMD64' -or $env:GITHUB_ACTIONS -eq 'true') {
    throw 'P028_APPROVED_WINDOWS_11_AMD64_REQUIRED'
}
$os = Get-CimInstance Win32_OperatingSystem
if ($os.Caption -notmatch 'Windows 11' -or [int]$os.BuildNumber -lt 22000) {
    throw 'P028_APPROVED_WINDOWS_11_AMD64_REQUIRED'
}
$uuid = [string](Get-CimInstance Win32_ComputerSystemProduct).UUID
if ([string]::IsNullOrWhiteSpace($uuid)) { throw 'P028_MACHINE_ID_UNAVAILABLE' }
$bytes = [Text.Encoding]::UTF8.GetBytes($uuid.ToLowerInvariant())
$machineHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
if ($machineHash -cne $ApprovedMachineUuidSha256) { throw 'P028_APPROVED_MACHINE_MISMATCH' }
& (Join-Path $PSScriptRoot 'qualify-windows-native-role-access.ps1') `
    -EvidenceDirectory $EvidenceDirectory `
    -OperatorApproval 'I_APPROVE_DISPOSABLE_SYNVEIL_NATIVE_ROLE_ACCESS' `
    -CandidatePath $CandidatePath -CandidateSha256 $CandidateSha256 `
    -ManifestPath $ManifestPath -ManifestSha256 $ManifestSha256 `
    -ExpectedSourceCommit $ExpectedSourceCommit `
    -ObserverCredential $ObserverCredential -SubjectCredential $SubjectCredential
if ($LASTEXITCODE -ne 0) { throw 'P028_ROLE_ACCESS_QUALIFICATION_FAILED' }
# Role probes do not establish the missing OS guarantees. Never report acceptance.
[ordered]@{
    mode='WINDOWS_NATIVE_QUALIFICATION'; native_acceptance='BLOCKED'
    role_evidence='REQUIRES_INDEPENDENT_REVIEW'
    interrupted_checkpoint_recovery='NOT_RUN_NATIVE'; session_event_binding='REQUIRES_INDEPENDENT_REVIEW'
    complete_resource_cleanup='NOT_RUN_NATIVE'; write_side_races='BLOCKED_UNQUALIFIED'
    native_authority='P028_NATIVE_AUTHORITY_UNAVAILABLE'
} | ConvertTo-Json -Depth 4
