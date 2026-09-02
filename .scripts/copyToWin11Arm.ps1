#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [string]$Destination
)

$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host 'copyToWin11Arm.ps1 [destination]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot
$projectFolder = Split-Path -Path $repoRoot -Leaf
if (-not $Destination) {
    if (-not $env:GITHUB_WIN11_ARM_COPY_ROOT) {
        throw 'Windows 11 ARM destination is not configured. Pass -Destination or load the user profile.'
    }
    $Destination = Join-Path $env:GITHUB_WIN11_ARM_COPY_ROOT $projectFolder
}

if (-not (Test-Path -LiteralPath $Destination -PathType Container)) {
    $parent = Split-Path -Path $Destination -Parent
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw "Windows 11 ARM destination is not reachable: $Destination"
    }
}

$excludedDirectories = @(
    (Join-Path $repoRoot '.git')
    (Join-Path $repoRoot 'target')
    (Join-Path $repoRoot 'publish')
    (Join-Path $repoRoot '.installer/Output')
    (Join-Path $repoRoot 'AppDir')
    (Join-Path $repoRoot 'AppImageTool')
)
$excludedFiles = @(
    '*.exe'
    '*.AppImage'
    '*.deb'
    '*.app'
    'rustitles_log.txt'
    'rustitles_settings.json'
    '*.code-workspace'
    '.DS_Store'
)
$arguments = @(
    $repoRoot
    $Destination
    '/E'
    '/IT'
    '/XJ'
    '/R:1'
    '/W:1'
    '/COPY:DAT'
    '/DCOPY:DAT'
    '/XD'
) + $excludedDirectories + @('/XF') + $excludedFiles

Write-Host "Copying project files to: $Destination"
& robocopy @arguments
if ($LASTEXITCODE -gt 7) { throw "robocopy failed (exit $LASTEXITCODE)." }

Write-Host 'Project files copied. Cargo target/cache and generated release output were excluded.'
closeOut 3
