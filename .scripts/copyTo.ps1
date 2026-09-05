#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [switch]$Debian,
    [switch]$Mac,
    [switch]$Win11Arm,
    [string]$Destination
)

$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host 'copyTo.ps1 [-debian] [-mac] [-win11arm] [destination]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot

$projectFolder = Split-Path -Path $repoRoot -Leaf
$selected = @()
if ($Debian) { $selected += 'Debian' }
if ($Mac) { $selected += 'Mac' }
if ($Win11Arm) { $selected += 'Win11Arm' }
if ($selected.Count -eq 0) { throw 'Specify at least one target: -Debian, -Mac, or -Win11Arm.' }
if ($Destination -and $selected.Count -gt 1) { throw '-Destination can only be used with a single target.' }

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

foreach ($target in $selected) {
    $envName = switch ($target) {
        'Debian' { 'GITHUB_DEBIAN_COPY_ROOT' }
        'Mac' { 'GITHUB_MAC_COPY_ROOT' }
        'Win11Arm' { 'GITHUB_WIN11_ARM_COPY_ROOT' }
    }
    $label = switch ($target) {
        'Debian' { 'Debian' }
        'Mac' { 'Mac' }
        'Win11Arm' { 'Windows 11 ARM' }
    }
    $errorMessage = switch ($target) {
        'Debian' { 'Debian destination is not configured. Pass -Destination or load the user profile.' }
        'Mac' { 'Mac destination is not configured. Pass -Destination or load the user profile.' }
        'Win11Arm' { 'Windows 11 ARM destination is not configured. Pass -Destination or load the user profile.' }
    }
    $envValue = [Environment]::GetEnvironmentVariable($envName)
    $dest = if ($Destination) { $Destination } elseif (-not $envValue) { throw $errorMessage } else { Join-Path $envValue $projectFolder }

    if (-not (Test-Path -LiteralPath $dest -PathType Container)) {
        $parent = Split-Path -Path $dest -Parent
        if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
            throw "$label destination is not reachable: $dest"
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
    if ($target -eq 'Debian') { $excludedDirectories += (Join-Path $repoRoot 'deb-pkg') }

    $copyFlags = switch ($target) {
        'Debian' { @('/COPY:DT', '/DCOPY:DT') }
        'Mac' { @('/COPY:DT', '/DCOPY:D') }
        default { @('/COPY:DAT', '/DCOPY:DAT') }
    }
    $arguments = @(
        $repoRoot
        $dest
        '/E'
        '/IT'
        '/XJ'
        '/R:1'
        '/W:1'
    ) + $copyFlags + @('/XD') + $excludedDirectories + @('/XF') + $excludedFiles

    Write-Host "Copying project files to: $dest ($label)"
    & robocopy @arguments
    if ($LASTEXITCODE -gt 7) { throw "robocopy failed ($label, exit $LASTEXITCODE)." }
}

Write-Host 'Project files copied. Cargo target/cache and generated release output were excluded.'
closeOut 3
