[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$OperatorApproval,
    [Parameter(Mandatory=$true)][string]$CandidatePath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$CandidateSha256,
    [Parameter(Mandatory=$true)][string]$ManifestPath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedSourceCommit,
    [PSCredential]$ObserverCredential,
    [PSCredential]$SubjectCredential
)

$ErrorActionPreference = 'Stop'
$approvalText = 'I_APPROVE_DISPOSABLE_SYNVEIL_NATIVE_ROLE_ACCESS'
$markerPath = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)) 'Synveil\P028\DISPOSABLE-ROLE-ACCESS-APPROVED.txt'
$programDataRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)) 'Synveil\P028'
$adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$systemSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$full = [uint32]0x1F01FF
$read = [uint32]0x1200A9
$modify = [uint32]0x1301BF
$createdRoot = $null
$children = @()
$processes = @()
$checkResults = [Collections.Generic.List[string]]::new()
$blockReason = 'P028_NATIVE_ROLE_ACCESS_NOT_RUN'
$roleResult = 'BLOCKED'
$evidenceOutputAllowed = $false
$platformWindows11 = 'NOT_RUN'
$platformAmd64 = 'NOT_RUN'
$platformBuild = 0
$candidateInspection = $null
$manifestInspection = $null
$candidateFileIdentitySha256 = ''
$manifestFileIdentitySha256 = ''
$harnessComponents = $null
$candidateSourceCommit = ''
$harnessSha256 = ''

function Assert-P028NoReparse([string]$Path) {
    $fullPath = [IO.Path]::GetFullPath($Path)
    $root = [IO.Path]::GetPathRoot($fullPath)
    $current = $root
    foreach ($part in $fullPath.Substring($root.Length).Split([IO.Path]::DirectorySeparatorChar, [StringSplitOptions]::RemoveEmptyEntries)) {
        $current = Join-Path $current $part
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'P028_ROLE_REPARSE_POINT_REJECTED' }
        }
    }
    return $fullPath
}

function Assert-P028ControllerOnlyDirectory([string]$Path, [bool]$AllowObserverRead = $false) {
    [void](Assert-P028NoReparse $Path)
    $acl = Get-Acl -LiteralPath $Path
    if (-not $acl.AreAccessRulesProtected -or $acl.GetOwner([Security.Principal.SecurityIdentifier]) -ne $adminSid) {
        throw 'P028_ROLE_EVIDENCE_DIRECTORY_ACL_INVALID'
    }
    $expected = @{$adminSid.Value=$full; $systemSid.Value=$full}
    if ($AllowObserverRead) { $expected[$observerSid.Value] = $read }
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    foreach ($rule in $rules) {
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or $rule.IsInherited -or
            $rule.InheritanceFlags -ne ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit) -or
            $rule.PropagationFlags -ne [Security.AccessControl.PropagationFlags]::None -or
            -not $expected.ContainsKey($rule.IdentityReference.Value) -or
            [uint32]$rule.FileSystemRights -ne [uint32]$expected[$rule.IdentityReference.Value]) {
            throw 'P028_ROLE_EVIDENCE_DIRECTORY_ACL_INVALID'
        }
        $expected.Remove($rule.IdentityReference.Value)
    }
    if ($expected.Count -ne 0) { throw 'P028_ROLE_EVIDENCE_DIRECTORY_ACL_INVALID' }
}

function Assert-P028ApprovalMarker([string]$Path) {
    [void](Assert-P028NoReparse $Path)
    $acl = Get-Acl -LiteralPath $Path
    if (-not $acl.AreAccessRulesProtected -or $acl.GetOwner([Security.Principal.SecurityIdentifier]) -ne $adminSid) {
        throw 'P028_DISPOSABLE_MARKER_ACL_INVALID'
    }
    $expected = @{$adminSid.Value=$full; $systemSid.Value=$full}
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    foreach ($rule in $rules) {
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or $rule.IsInherited -or
            -not $expected.ContainsKey($rule.IdentityReference.Value) -or
            [uint32]$rule.FileSystemRights -ne [uint32]$expected[$rule.IdentityReference.Value]) {
            throw 'P028_DISPOSABLE_MARKER_ACL_INVALID'
        }
        $expected.Remove($rule.IdentityReference.Value)
    }
    if ($expected.Count -ne 0) { throw 'P028_DISPOSABLE_MARKER_ACL_INVALID' }
}

function Set-P028DirectoryAcl([string]$Path, [hashtable]$Entries) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner($adminSid)
    $inherit = [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit
    foreach ($entry in $Entries.GetEnumerator()) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($entry.Key),
            [Security.AccessControl.FileSystemRights]$entry.Value,
            $inherit, [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
    $actual = Get-Acl -LiteralPath $Path
    if (-not $actual.AreAccessRulesProtected -or $actual.GetOwner([Security.Principal.SecurityIdentifier]) -ne $adminSid) {
        throw 'P028_ROLE_TEST_ACL_COMMIT_FAILED'
    }
    $actualRules = @($actual.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    if ($actualRules.Count -ne $Entries.Count) { throw 'P028_ROLE_TEST_ACL_COMMIT_FAILED' }
    foreach ($rule in $actualRules) {
        if ($rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or $rule.IsInherited -or
            -not $Entries.ContainsKey($rule.IdentityReference.Value) -or
            [uint32]$rule.FileSystemRights -ne [uint32]$Entries[$rule.IdentityReference.Value]) {
            throw 'P028_ROLE_TEST_ACL_COMMIT_FAILED'
        }
    }
}

function Start-P028RoleProbe([string]$Role, [Security.Principal.SecurityIdentifier]$Sid,
                             [PSCredential]$Credential, [string]$Probe, [string]$ObserverInbox,
                             [string]$SubjectInbox, [string]$EvidenceFile,
                             [string]$StateFile, [string]$Nonce, [string]$ReleaseFile) {
    $parts = $Credential.UserName -split '\\', 2
    $domain = if ($parts.Count -eq 2) { $parts[0] } else { [Environment]::MachineName }
    $user = if ($parts.Count -eq 2) { $parts[1] } else { $parts[0] }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $PSHOME 'pwsh.exe'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.UserName = $user
    $info.Domain = $domain
    $info.Password = $Credential.Password
    $info.LoadUserProfile = $false
    $info.WorkingDirectory = $createdRoot
    foreach ($arg in @('-NoLogo','-NoProfile','-NonInteractive','-File',$Probe,
        '-Role',$Role,'-ExpectedSid',$Sid.Value,'-ObserverInbox',$ObserverInbox,
        '-SubjectInbox',$SubjectInbox,'-EvidenceFile',$EvidenceFile,
        '-ProtectedStateFile',$StateFile,'-Nonce',$Nonce,'-ReleaseFile',$ReleaseFile)) {
        $info.ArgumentList.Add([string]$arg)
    }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    if (-not $process.Start()) { throw 'P028_ROLE_PROBE_START_FAILED' }
    return $process
}

function Wait-P028RoleProbe([Diagnostics.Process]$Process) {
    if (-not $Process.WaitForExit(30000)) {
        try { $Process.Kill($true) } catch { }
        throw 'P028_ROLE_PROBE_TIMEOUT'
    }
    $stderr = $Process.StandardError.ReadToEnd()
    [void]$Process.StandardOutput.ReadToEnd()
    if ($Process.ExitCode -ne 0) {
        if ($stderr -match 'P028_[A-Z0-9_]+') { throw $Matches[0] }
        throw 'P028_ROLE_PROBE_FAILED'
    }
}

function Assert-P028Access([uint32]$Granted, [uint32]$Expected) {
    if (($Granted -band $Expected) -ne $Expected -or ($Granted -band ($full -bxor $Expected)) -ne 0) {
        throw 'P028_ROLE_EFFECTIVE_ACCESS_MISMATCH'
    }
}

function Test-P028ActualAccess([string]$Path, [uint32]$ControllerPid,
                               [Diagnostics.Process]$ObserverProcess,
                               [Diagnostics.Process]$SubjectProcess,
                               [uint32]$ObserverExpected, [uint32]$SubjectExpected) {
    [void](Assert-P028NoReparse $Path)
    $descriptor = (Get-Acl -LiteralPath $Path).GetSecurityDescriptorBinaryForm()
    Assert-P028Access ([P028ObserverAccess]::MaximumAccess($ControllerPid, $controllerSid.Value, 'Controller', $descriptor)) $full
    Assert-P028Access ([P028ObserverAccess]::MaximumAccess([uint32]$ObserverProcess.Id, $observerSid.Value, 'Observer', $descriptor)) $ObserverExpected
    Assert-P028Access ([P028ObserverAccess]::MaximumAccess([uint32]$SubjectProcess.Id, $subjectSid.Value, 'Subject', $descriptor)) $SubjectExpected
}

function Save-P028RedactedEvidence([string]$Directory, [hashtable]$Record) {
    $nonce = [Guid]::NewGuid().ToString('N')
    $destination = Join-Path $Directory ('role-access-' + $nonce + '.json')
    $temporary = Join-Path $Directory ('.role-access-' + $nonce + '.tmp')
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Record | ConvertTo-Json -Depth 5 -Compress) + "`n")
    if ($bytes.Length -gt 16384) { throw 'P028_ROLE_EVIDENCE_TOO_LARGE' }
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
    try { [IO.File]::Move($temporary, $destination) }
    catch { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue; throw 'P028_ROLE_EVIDENCE_COMMIT_FAILED' }
    return $destination
}

function Get-P028NativeQualificationIdentity {
    $candidate = Assert-P028NoReparse $CandidatePath
    $manifest = Assert-P028NoReparse $ManifestPath
    $candidateInfo = [P028SafeEvidenceReader]::Inspect($candidate, 536870912)
    $manifestInfo = [P028SafeEvidenceReader]::Inspect($manifest, 1048576)
    $manifestBytes = [P028SafeEvidenceReader]::Read($manifest, 1048576)
    $manifestReadSha = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($manifestBytes)).ToLowerInvariant()
    if ($candidateInfo.Sha256 -cne $CandidateSha256 -or $manifestInfo.Sha256 -cne $ManifestSha256 -or
        $manifestReadSha -cne $manifestInfo.Sha256) {
        throw 'P028_CANDIDATE_IDENTITY_MISMATCH'
    }
    try {
        $release = [Text.Encoding]::UTF8.GetString($manifestBytes) | ConvertFrom-Json
    } catch { throw 'P028_CANDIDATE_MANIFEST_INVALID' }
    if ([int]$release.schema_version -ne 1 -or
        $release.source_commit -cne $ExpectedSourceCommit -or
        $release.product -cne 'Synveil' -or $null -eq $release.artifacts) {
        throw 'P028_CANDIDATE_MANIFEST_CONTEXT_MISMATCH'
    }
    $installer = @($release.artifacts | Where-Object {
        $_.id -ceq 'windows-x86_64-installer' -and $_.filename -ceq 'SynveilSetup.exe'
    })
    if ($installer.Count -ne 1 -or $installer[0].artifact_type -cne 'windows_installer' -or
        $installer[0].platform -cne 'windows' -or $installer[0].architecture -cne 'x86_64' -or
        $installer[0].role -cne 'primary_installer' -or
        $installer[0].sha256 -cne $candidateInfo.Sha256 -or
        [long]$installer[0].size_bytes -ne $candidateInfo.Length) {
        throw 'P028_CANDIDATE_MANIFEST_ARTIFACT_MISMATCH'
    }
    $components = [ordered]@{}
    foreach ($component in @(
        @{Name='role_qualifier'; Path=$PSCommandPath},
        @{Name='role_probe'; Path=(Join-Path $PSScriptRoot 'windows-native-role-probe.ps1')},
        @{Name='observer_support'; Path=(Join-Path $PSScriptRoot 'windows-native-observer-support.ps1')}
    )) {
        $componentPath = Assert-P028NoReparse $component.Path
        $components[$component.Name] = [P028SafeEvidenceReader]::Inspect($componentPath, 1048576).Sha256
    }
    $aggregateText = (($components.GetEnumerator() | ForEach-Object { $_.Key + ':' + $_.Value }) -join "`n")
    $aggregate = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($aggregateText))).ToLowerInvariant()
    return [pscustomobject]@{
        CandidateSha256=$candidateInfo.Sha256; CandidateSize=$candidateInfo.Length
        CandidateFileIdentitySha256=$candidateInfo.FileIdentitySha256
        ManifestSha256=$manifestInfo.Sha256; SourceCommit=$release.source_commit
        ManifestFileIdentitySha256=$manifestInfo.FileIdentitySha256
        HarnessComponents=$components; HarnessSha256=$aggregate
    }
}

try {
    if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'P028_REQUIRES_WINDOWS_POWERSHELL_7' }
    if ($OperatorApproval -cne $approvalText) { throw 'P028_ROLE_ACCESS_APPROVAL_REQUIRED' }
    if ($env:GITHUB_ACTIONS -eq 'true' -or $env:RUNNER_NAME -or $env:CI -eq 'true') { throw 'P028_ROLE_ACCESS_CI_HOST_BLOCKED' }
    $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    $platformWindows11 = if ($os.Caption -match 'Windows 11' -and [int]$os.BuildNumber -ge 22000) { 'PASS' } else { 'FAIL' }
    $platformAmd64 = if ($env:PROCESSOR_ARCHITECTURE -ceq 'AMD64' -and [Environment]::Is64BitOperatingSystem) { 'PASS' } else { 'FAIL' }
    $platformBuild = [int]$os.BuildNumber
    if ($platformWindows11 -ne 'PASS' -or $platformAmd64 -ne 'PASS') {
        throw 'P028_NATIVE_PLATFORM_UNSUPPORTED'
    }
    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) { throw 'P028_DISPOSABLE_MARKER_MISSING' }
    Assert-P028ApprovalMarker $markerPath
    $marker = Get-Content -LiteralPath $markerPath -Raw
    if ($marker.Trim() -cne 'SYNVEIL_DISPOSABLE_ROLE_ACCESS_APPROVED') { throw 'P028_DISPOSABLE_MARKER_INVALID' }

    . (Join-Path $PSScriptRoot 'windows-native-observer-support.ps1')
    $qualificationIdentity = Get-P028NativeQualificationIdentity
    $candidateInspection = $qualificationIdentity.CandidateSha256
    $manifestInspection = $qualificationIdentity.ManifestSha256
    $candidateFileIdentitySha256 = $qualificationIdentity.CandidateFileIdentitySha256
    $manifestFileIdentitySha256 = $qualificationIdentity.ManifestFileIdentitySha256
    $candidateSourceCommit = $qualificationIdentity.SourceCommit
    $harnessComponents = $qualificationIdentity.HarnessComponents
    $harnessSha256 = $qualificationIdentity.HarnessSha256

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $controllerSid = $identity.User
        $controllerPrincipal = [Security.Principal.WindowsPrincipal]::new($identity)
        if (-not $controllerPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -or
            -not [P028ObserverAccess]::IsElevated([uint32]$PID, $controllerSid.Value)) {
            throw 'P028_CONTROLLER_TOKEN_NOT_ELEVATED'
        }
        [P028ObserverAccess]::AssertControllerToken([uint32]$PID, $controllerSid.Value)
    } finally { $identity.Dispose() }

    $evidenceDirectory = Assert-P028NoReparse $EvidenceDirectory
    $approvedRoot = [IO.Path]::GetFullPath($programDataRoot)
    if (-not (Test-Path -LiteralPath $evidenceDirectory -PathType Container) -or
        $evidenceDirectory -ine $approvedRoot -or -not (Test-Path -LiteralPath $approvedRoot -PathType Container)) {
        throw 'P028_ROLE_EVIDENCE_DIRECTORY_INVALID'
    }
    [void](Assert-P028NoReparse $approvedRoot)

    $observerAccount = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount=TRUE" -ErrorAction Stop | Where-Object { $_.SID -match '-[0-9]+$' -and $_.Name -match '^synveil_obs_[0-9a-f]{8}$' })
    $subjectAccount = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount=TRUE" -ErrorAction Stop | Where-Object { $_.SID -match '-[0-9]+$' -and $_.Name -match '^synveil_p013_[0-9a-f]{8}$' -and $_.Description -ceq 'SYNVEIL P013 DISPOSABLE ACCEPTANCE ACCOUNT' })
    if ($observerAccount.Count -ne 1 -or $subjectAccount.Count -ne 1) { throw 'P028_ROLE_DISPOSABLE_ACCOUNTS_INVALID' }
    $observerSid = [Security.Principal.SecurityIdentifier]::new([string]$observerAccount[0].SID)
    $subjectSid = [Security.Principal.SecurityIdentifier]::new([string]$subjectAccount[0].SID)
    if ($controllerSid -eq $observerSid -or $controllerSid -eq $subjectSid -or $observerSid -eq $subjectSid) { throw 'P028_ROLE_IDENTITY_COLLISION' }
    Assert-P028ControllerOnlyDirectory $approvedRoot $false
    $evidenceOutputAllowed = $true
    if (-not $ObserverCredential) { $ObserverCredential = Get-Credential -UserName ("$([Environment]::MachineName)\$($observerAccount[0].Name)") -Message 'Disposable observer account; password is used only to start the role probe.' }
    if (-not $SubjectCredential) { $SubjectCredential = Get-Credential -UserName ("$([Environment]::MachineName)\$($subjectAccount[0].Name)") -Message 'Disposable subject account; password is used only to start the role probe.' }
    foreach ($pair in @(@($ObserverCredential,$observerSid),@($SubjectCredential,$subjectSid))) {
        try { $credentialSid = ([Security.Principal.NTAccount]::new($pair[0].UserName)).Translate([Security.Principal.SecurityIdentifier]) }
        catch { throw 'P028_ROLE_CREDENTIAL_IDENTITY_INVALID' }
        if ($credentialSid -ne $pair[1]) { throw 'P028_ROLE_CREDENTIAL_IDENTITY_INVALID' }
    }

    $drive = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($approvedRoot))
    if ($drive.DriveType -ne [IO.DriveType]::Fixed -or $drive.DriveFormat -cne 'NTFS') { throw 'P028_ROLE_STATE_REQUIRES_LOCAL_NTFS' }
    $nonce = [Guid]::NewGuid().ToString('N')
    $createdRoot = Join-Path $approvedRoot ('role-qualification-' + $nonce)
    [void][IO.Directory]::CreateDirectory($createdRoot)
    $stateRoot = Join-Path $createdRoot 'controller-state'
    $evidenceRoot = Join-Path $createdRoot 'evidence'
    $observerInbox = Join-Path $createdRoot 'observer-inbox'
    $subjectInbox = Join-Path $createdRoot 'subject-inbox'
    foreach ($directory in @($stateRoot,$evidenceRoot,$observerInbox,$subjectInbox)) {
        [void][IO.Directory]::CreateDirectory($directory)
    }
    $rootEntries = @{$adminSid.Value=$full; $systemSid.Value=$full; $observerSid.Value=$read; $subjectSid.Value=$read}
    $stateEntries = @{$adminSid.Value=$full; $systemSid.Value=$full}
    $evidenceEntries = @{$adminSid.Value=$full; $systemSid.Value=$full; $observerSid.Value=$read}
    $observerEntries = @{$adminSid.Value=$full; $systemSid.Value=$full; $observerSid.Value=$modify}
    $subjectEntries = @{$adminSid.Value=$full; $systemSid.Value=$full; $subjectSid.Value=$modify}
    Set-P028DirectoryAcl $createdRoot $rootEntries
    Set-P028DirectoryAcl $stateRoot $stateEntries
    Set-P028DirectoryAcl $evidenceRoot $evidenceEntries
    Set-P028DirectoryAcl $observerInbox $observerEntries
    Set-P028DirectoryAcl $subjectInbox $subjectEntries

    $stateFile = Join-Path $stateRoot 'controller-state.json'
    $evidenceFile = Join-Path $evidenceRoot 'approved-evidence.json'
    $subjectSeed = Join-Path $subjectInbox 'existing.txt'
    $probe = Join-Path $createdRoot 'windows-native-role-probe.ps1'
    $support = Join-Path $createdRoot 'windows-native-observer-support.ps1'
    $observerRelease = Join-Path $createdRoot 'release-observer'
    $subjectRelease = Join-Path $createdRoot 'release-subject'
    [IO.File]::WriteAllText($stateFile, 'CONTROLLER-' + $nonce)
    [IO.File]::WriteAllText($evidenceFile, 'EVIDENCE-' + $nonce)
    [IO.File]::WriteAllText($subjectSeed, 'SUBJECT-SEED-' + $nonce)
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows-native-role-probe.ps1') -Destination $probe
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows-native-observer-support.ps1') -Destination $support

    # Exercise controller-authorized I/O through the OS, then start each role
    # under its own credential for real effective-token and access checks.
    [IO.File]::WriteAllText($stateFile, 'CONTROLLER-WRITE-' + $nonce)
    if ([IO.File]::ReadAllText($evidenceFile) -cne ('EVIDENCE-' + $nonce)) { throw 'P028_CONTROLLER_IO_FAILURE' }
    [IO.File]::WriteAllText((Join-Path $observerInbox 'controller-submission.json'), 'CONTROLLER-SUBMISSION')
    [IO.File]::WriteAllText((Join-Path $subjectInbox 'controller-submission.txt'), 'CONTROLLER-SUBMISSION')
    $checkResults.Add('controller_file_operations')

    $processes += Start-P028RoleProbe 'Observer' $observerSid $ObserverCredential $probe $observerInbox $subjectInbox $evidenceFile $stateFile $nonce $observerRelease
    $processes += Start-P028RoleProbe 'Subject' $subjectSid $SubjectCredential $probe $observerInbox $subjectInbox $evidenceFile $stateFile $nonce $subjectRelease
    Start-Sleep -Milliseconds 500
    foreach ($process in $processes) {
        if ($process.HasExited) { throw 'P028_ROLE_PROBE_EARLY_EXIT' }
    }

    $policies = @(
        @{Path=$createdRoot; Observer=$read; Subject=$read},
        @{Path=$stateRoot; Observer=[uint32]0; Subject=[uint32]0},
        @{Path=$stateFile; Observer=[uint32]0; Subject=[uint32]0},
        @{Path=$evidenceRoot; Observer=$read; Subject=[uint32]0},
        @{Path=$evidenceFile; Observer=$read; Subject=[uint32]0},
        @{Path=$observerInbox; Observer=$modify; Subject=[uint32]0},
        @{Path=$subjectInbox; Observer=[uint32]0; Subject=$modify}
    )
    foreach ($item in $policies) {
        Test-P028ActualAccess $item.Path ([uint32]$PID) $processes[0] $processes[1] $item.Observer $item.Subject
    }
    $checkResults.Add('actual_token_access_check')
    [IO.File]::WriteAllText($observerRelease, 'release')
    [IO.File]::WriteAllText($subjectRelease, 'release')
    foreach ($process in $processes) { Wait-P028RoleProbe $process }
    $checkResults.Add('observer_and_subject_os_file_operations')
    if ([IO.File]::ReadAllText($stateFile) -cne ('CONTROLLER-WRITE-' + $nonce)) { throw 'P028_PROTECTED_STATE_WAS_MODIFIED' }
    $checkResults.Add('protected_state_unchanged')
    $roleResult = 'PASS'
} catch {
    $blockReason = $_.Exception.Message
    if ($blockReason -notmatch '^P028_[A-Z0-9_]+$') { $blockReason = 'P028_NATIVE_ROLE_ACCESS_BLOCKED' }
} finally {
    foreach ($process in $processes) {
        if ($process -and -not $process.HasExited) {
            try { $process.Kill($true); $process.WaitForExit(5000) | Out-Null } catch { $roleResult = 'BLOCKED'; $blockReason = 'P028_ROLE_PROBE_CLEANUP_FAILED' }
        }
        if ($process) { $process.Dispose() }
    }
    if ($createdRoot) {
        try {
            [void](Assert-P028NoReparse $createdRoot)
            $reparse = @(Get-ChildItem -LiteralPath $createdRoot -Force -Recurse | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 })
            if ($reparse.Count -ne 0) { throw 'P028_ROLE_TEST_CLEANUP_REPARSE_FOUND' }
            Remove-Item -LiteralPath $createdRoot -Recurse -Force
            if (Test-Path -LiteralPath $createdRoot) { throw 'P028_ROLE_TEST_CLEANUP_INCOMPLETE' }
        } catch {
            $roleResult = 'BLOCKED'
            $blockReason = 'P028_ROLE_TEST_CLEANUP_INCOMPLETE'
        }
    }
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try {
    $machineHash = ''
    try {
        $machineGuid = [string](Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography' -Name MachineGuid)
        $machineHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($machineGuid))).ToLowerInvariant()
    } catch { $machineHash = '' }
    $record = [ordered]@{
        schema_version = 1
        result = $roleResult
        native_acceptance = 'BLOCKED_NOT_RUN'
        blocked_reason = if ($roleResult -eq 'PASS') { 'P028_NATIVE_INTERACTIVE_QUALIFICATION_NOT_RUN' } else { $blockReason }
        executed_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        candidate_sha256 = $candidateInspection
        candidate_file_identity_sha256 = $candidateFileIdentitySha256
        candidate_manifest_sha256 = $manifestInspection
        manifest_file_identity_sha256 = $manifestFileIdentitySha256
        candidate_source_commit = $candidateSourceCommit
        harness_sha256 = if ($harnessSha256) { $harnessSha256 } else { '' }
        harness_components_sha256 = $harnessComponents
        machine_sha256 = $machineHash
        platform = [ordered]@{ windows_11 = $platformWindows11; amd64 = $platformAmd64; build = $platformBuild }
        controller_sid_sha256 = if ($identity.User) { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).ToLowerInvariant() } else { '' }
        role_checks = @($checkResults.ToArray())
        temporary_acl_tree_removed = -not ($createdRoot -and (Test-Path -LiteralPath $createdRoot))
        redaction = [ordered]@{ raw_sids = $false; account_names = $false; credentials = $false; file_paths = $false; event_xml = $false }
    }
} finally { $identity.Dispose() }
try {
    if (-not $evidenceOutputAllowed) {
        [Console]::Error.WriteLine($blockReason)
        exit 2
    }
    $evidencePath = Save-P028RedactedEvidence $EvidenceDirectory $record
} catch {
    [Console]::Error.WriteLine('P028_ROLE_EVIDENCE_COMMIT_FAILED')
    exit 2
}
Write-Output ("P028_ROLE_ACCESS_{0} evidence_sha256={1}" -f $roleResult, (Get-FileHash -LiteralPath $evidencePath -Algorithm SHA256).Hash.ToLowerInvariant())
if ($roleResult -ne 'PASS') { exit 2 }
