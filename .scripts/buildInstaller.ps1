#requires -Version 7.0
param([Alias('h')][switch]$Help, [string]$Architecture)
$ErrorActionPreference = 'Stop'
if ($Help) {
    Write-Host 'buildInstaller.ps1 [-Architecture x64|arm64]'
    Write-Host 'Windows: portable executables and Inno Setup installers.'
    Write-Host 'Linux: native Debian AppImage and .deb packages.'
    Write-Host 'macOS: native universal app and ZIP archive.'
    return
}
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Write-Host "--- packaging $projectName... ---"
Set-Location -LiteralPath $repoRoot
removeRootReleaseArtifacts
$targetArchitecture = getArchitecture @($Architecture)
$previousPipeline = $env:BASE_BUILD_PIPELINE
try {
    $env:BASE_BUILD_PIPELINE = '1'
    invokeHostPackagers -Architecture $targetArchitecture
} finally { $env:BASE_BUILD_PIPELINE = $previousPipeline }
closeOut 3
