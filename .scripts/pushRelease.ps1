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

function assertReleaseTargetAvailable {
    $localTagOutput = & git show-ref --verify --quiet "refs/tags/$tag" 2>&1
    $localTagExit = $LASTEXITCODE
    if ($localTagExit -eq 0) { throw "Local tag already exists: $tag" }
    if ($localTagExit -ne 1) { throw "Could not check the local tag: $tag" }

    $remoteTagOutput = & git ls-remote --exit-code --refs origin "refs/tags/$tag" 2>&1
    $remoteTagExit = $LASTEXITCODE
    if ($remoteTagExit -eq 0) { throw "Remote tag already exists: $tag" }
    if ($remoteTagExit -ne 2) {
        $details = ($remoteTagOutput -join ' ').Trim()
        throw "Could not check the remote tag: $tag$(if ($details) { ": $details" })"
    }

    $releaseOutput = @(& gh release view $tag --repo $ghRepo --json tagName 2>&1)
    $releaseExit = $LASTEXITCODE
    if ($releaseExit -eq 0) { throw "GitHub release already exists: $tag" }
    if ($releaseExit -ne 1) {
        $details = ($releaseOutput -join ' ').Trim()
        throw "Could not check the GitHub release: $tag$(if ($details) { ": $details" })"
    }
    $releaseText = ($releaseOutput -join ' ')
    if ($releaseText -and $releaseText -notmatch '(?i)(not found|could not find|404|does not exist)') {
        throw "Could not determine whether the GitHub release exists: $releaseText"
    }
}

assertReleaseTargetAvailable
$assetArgs = @($assets | ForEach-Object FullName)
$notes = readBuildNotes
$title = if ($notes.Title) { $notes.Title } else { $tag }
$releaseNotes = $notes.Body
$releaseArgs = @('release', 'create', $tag, '--title', $title, '--repo', $ghRepo)
if ($releaseNotes) { $releaseArgs += @('--notes', $releaseNotes) } else { $releaseArgs += '--generate-notes' }
if ($DryRun) {
    Write-Host "DRY RUN: preflight confirmed tag/release target is unused: $tag"
    Write-Host "DRY RUN: git tag $tag"
    Write-Host "DRY RUN: git push origin refs/tags/$tag"
    $quotedArgs = @($releaseArgs + $assetArgs | ForEach-Object { if ($_ -match '\s') { "'$_'" } else { $_ } }) -join ' '
    Write-Host "DRY RUN: gh $quotedArgs"
    Write-Host "DRY RUN: $appURL/releases/tag/$tag"
    return
}
# Guard against tagging an uncommitted version/buildNotes state - only for real push.
& git @('diff', '--quiet') 2>$null; if ($LASTEXITCODE) { throw 'Working tree has uncommitted changes. Commit or stash before pushRelease.' }
& git @('diff', '--cached', '--quiet') 2>$null; if ($LASTEXITCODE) { throw 'Staged but uncommitted changes present. Commit before pushRelease.' }
runNativeCommand git @('tag', $tag) 'git tag'
runNativeCommand git @('push', 'origin', "refs/tags/$tag") 'git push tag'
runNativeCommand gh ($releaseArgs + $assetArgs) 'gh release create'
openUrl "$appURL/releases/tag/$tag"
closeOut 3
