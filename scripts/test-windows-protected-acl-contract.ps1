# Safe in-memory ACL contract checks; no account or filesystem ACL changes.
$ErrorActionPreference = 'Stop'
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$systemSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$subject = [Security.Principal.SecurityIdentifier]::new('S-1-5-21-1-2-3-1001')
$script:acls = @{}
function Set-Acl { param($LiteralPath, $AclObject) $script:acls[$LiteralPath] = $AclObject }
function Get-Acl { param($LiteralPath) return $script:acls[$LiteralPath] }
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'protect-windows-native-run-state.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'P028_TEST_PARSE_FAILED' }
foreach ($name in @('Get-ExpectedAcl','Set-RunAcl')) {
    $fn = $ast.FindAll({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
    if ($fn.Count -ne 1) { throw 'P028_TEST_FUNCTION_MISSING' }
    Invoke-Expression $fn[0].Extent.Text
}
function Assert-Failure([scriptblock]$Action, [string]$Expected) {
    try { & $Action } catch { if ($_.Exception.Message -eq $Expected) { return }; throw }
    throw "P028_TEST_EXPECTED_FAILURE: $Expected"
}
Set-RunAcl 'observer' $false $false
Set-RunAcl 'evidence' $false $false
Set-RunAcl 'subject-inbox' $true $true
Set-RunAcl 'trusted' $false $true
Get-ExpectedAcl 'observer' $false $false
Get-ExpectedAcl 'evidence' $false $false
Get-ExpectedAcl 'subject-inbox' $true $true
Get-ExpectedAcl 'trusted' $false $true
Assert-Failure { Get-ExpectedAcl 'observer' $false $true } 'P028_REQUIRED_ACL_MISSING'
Assert-Failure { Get-ExpectedAcl 'subject-inbox' $true $false } 'P028_UNEXPECTED_ACL_RULE'
$script:acls['observer'].SetOwner($subject)
Assert-Failure { Get-ExpectedAcl 'observer' $false $false } 'P028_STATE_OWNER_MISMATCH'
$script:acls['observer'].SetOwner($adminSid)
$script:acls['observer'].SetAccessRuleProtection($false, $false)
Assert-Failure { Get-ExpectedAcl 'observer' $false $false } 'P028_INHERITED_ACL_REJECTED'
$script:acls['observer'].SetAccessRuleProtection($true, $false)
$rule = [Security.AccessControl.FileSystemAccessRule]::new($subject, [Security.AccessControl.FileSystemRights]::Modify, ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit), [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
[void]$script:acls['observer'].AddAccessRule($rule)
Assert-Failure { Get-ExpectedAcl 'observer' $false $false } 'P028_UNEXPECTED_ACL_RULE'
Write-Output 'P028_ACL_CONTRACT_TESTS_PASS: 4 policies, 5 negative checks (in-memory only)'
