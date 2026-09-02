#requires -Version 7.0
param([Alias('n')][switch]$DryRun)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot
assertVersionConsistency
if (-not (Test-Path -LiteralPath $publishFolder -PathType Container)) { throw "Missing release asset folder: $publishFolder" }
# Validate staged artifacts before tagging; -File intentionally skips publish/rustitles.app container and publish/build intermediates (zip is the mac artifact).
assertPublishArtifacts
$expectedNames = @(
    (rustitlesWinExeName 'x64')
    (rustitlesWinExeName 'arm64')
    (rustitlesWinInstallerName 'x64')
    (rustitlesWinInstallerName 'arm64')
    (rustitlesMacZipName)
    (rustitlesLinuxAppImageName)
    (rustitlesLinuxDebName)
)
# Only top-level files, filtered to allowlist so stray logs/DS_Store are not uploaded.
$assets = @(Get-ChildItem -LiteralPath $publishFolder -File | Where-Object { $_.Name -in $expectedNames })
if (-not $assets) { throw "No release assets found in $publishFolder" }
$missing = @($expectedNames | Where-Object { $_ -notin $assets.Name })
if ($missing) { throw "Missing expected publish assets: $($missing -join ', ')" }
$extra = @((Get-ChildItem -LiteralPath $publishFolder -File) | Where-Object { $_.Name -notin $expectedNames } | ForEach-Object Name)
if ($extra) { throw "Unexpected files in publish (remove or update allowlist): $($extra -join ', ')" }
$assetArgs = @($assets | ForEach-Object FullName)
$notes = readBuildNotes
$title = if ($notes.Title) { $notes.Title } else { $tag }
$releaseNotes = $notes.Body
$releaseArgs = @('release', 'create', $tag, '--title', $title, '--repo', $ghRepo)
if ($releaseNotes) { $releaseArgs += @('--notes', $releaseNotes) } else { $releaseArgs += '--generate-notes' }
if ($DryRun) {
    Write-Host "DRY RUN: git tag -f $tag"
    Write-Host "DRY RUN: git push origin refs/tags/$tag --force"
    $quotedArgs = @($releaseArgs + $assetArgs | ForEach-Object { if ($_ -match '\s') { "'$_'" } else { $_ } }) -join ' '
    Write-Host "DRY RUN: gh $quotedArgs"
    Write-Host "DRY RUN: $appURL/releases/tag/$tag"
    return
}
# Guard against tagging an uncommitted version/buildNotes state - only for real push.
& git @('diff', '--quiet') 2>$null; if ($LASTEXITCODE) { throw 'Working tree has uncommitted changes. Commit or stash before pushRelease.' }
& git @('diff', '--cached', '--quiet') 2>$null; if ($LASTEXITCODE) { throw 'Staged but uncommitted changes present. Commit before pushRelease.' }
runNativeCommand git @('tag', '-f', $tag) 'git tag'
runNativeCommand git @('push', 'origin', "refs/tags/$tag", '--force') 'git push tag'
runNativeCommand gh ($releaseArgs + $assetArgs) 'gh release create'
openUrl "$appURL/releases/tag/$tag"
closeOut 3
