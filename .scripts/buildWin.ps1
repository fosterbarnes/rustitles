#requires -Version 7.0
param(
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')

$cargoTarget = getWindowsCargoTarget $Architecture
$releaseEXE = Join-Path $repoRoot "target/$cargoTarget/release/rustitles.exe"
$target = getWindowsInstallerTargets $Architecture
$finalExePath = Join-Path $publishFolder (rustitlesWinExeName $Architecture)

assertVersionConsistency
setVerBuild $Architecture

removeRootReleaseArtifacts
Get-ChildItem -LiteralPath $repoRoot -Filter 'rustitles_lo*.txt' -File -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force }
deleteDir $target.BinFolder
New-Item -ItemType Directory -Path $target.BinFolder -Force | Out-Null
Remove-Item -LiteralPath $finalExePath -Force -ErrorAction SilentlyContinue

Write-Host "Building $Architecture ($cargoTarget)..."
runNativeCommand cargo @('build', '--release', '--target', $cargoTarget, '--manifest-path', $crateManifest) "cargo build $Architecture"

if (-not (Test-Path -LiteralPath $releaseEXE -PathType Leaf)) {
    throw "Release executable not found: $releaseEXE"
}
Copy-Item -LiteralPath $releaseEXE -Destination $target.ExePath -Force
Copy-Item -LiteralPath $releaseEXE -Destination $finalExePath -Force
Write-Host "Executable staged to: $finalExePath"
Write-Host 'Build completed successfully!'
closeOut 3
