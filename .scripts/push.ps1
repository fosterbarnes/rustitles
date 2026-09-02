#requires -Version 7.0
param(
    [Alias('f')][switch]$Force,
    [Alias('n')][switch]$DryRun,
    [Parameter(Position = 0)][string]$Message
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot
$notes = readBuildNotes
$commitMessage = $notes.Title
if (-not $commitMessage) { $commitMessage = $Message.Trim() }
if (-not $commitMessage) { throw 'Provide a commit message or add a non-empty first line to buildNotes.txt.' }
$commitBody = $notes.Body
$branch = (& git branch --show-current).Trim()
if ($LASTEXITCODE) { throw 'Could not determine the current branch.' }
if (-not $branch) { throw 'Detached HEAD; refusing to push.' }
$commitArgs = @('commit', '-m', $commitMessage)
if ($commitBody) { $commitArgs += @('-m', $commitBody) }
$pushArgs = @('push', 'origin', $branch); if ($Force) { $pushArgs += '--force' }
if ($DryRun) {
    Write-Host "DRY RUN: git add -A"
    Write-Host "DRY RUN: git $($commitArgs -join ' ')"
    Write-Host "DRY RUN: git $($pushArgs -join ' ')"
    Write-Host "DRY RUN: $appURL"
    return
}
runNativeCommand git @('add', '-A') 'git add'
runNativeCommand git $commitArgs 'git commit'
runNativeCommand git $pushArgs 'git push'
openUrl $appURL
closeOut 3
