# Resolve the current token's Windows namespace even before a new profile's
# shell has created Desktop/Start Menu folders. Do not create common folders.
function Get-WindowsKnownFolderPath([System.Environment+SpecialFolder]$Folder) {
    $path = [Environment]::GetFolderPath($Folder, [System.Environment+SpecialFolderOption]::DoNotVerify)
    if ([string]::IsNullOrWhiteSpace($path) -or ![IO.Path]::IsPathFullyQualified($path)) {
        throw "WINDOWS_PROFILE_FAILURE: Windows known folder $Folder is unavailable"
    }
    return $path
}
