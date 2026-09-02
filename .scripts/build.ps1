#requires -Version 7.0
param([Alias('h')][switch]$Help, [string]$Architecture)
$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host 'build.ps1 [-Architecture x64|arm64]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Write-Host "=== building $projectName... ==="
Set-Location -LiteralPath $repoRoot
assertVersionConsistency
checkVerBuild $Architecture
$targetArchitecture = getArchitecture @($Architecture)
$cargoArgs = @('build', '--release', '--manifest-path', $manifest)
if ($IsWindows -and $targetArchitecture) {
    $cargoArgs += @('--target', (getWindowsCargoTarget $targetArchitecture))
    setVerBuild $targetArchitecture
}
runNativeCommand cargo $cargoArgs 'cargo build --release'
closeOut 3
