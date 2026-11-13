#requires -Version 7.0
param(
    [Alias('f')][switch]$Force,
    [Alias('n')][switch]$DryRun,
    [Parameter(Position = 0)][string]$Message
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot

# buildNotes.txt holds the next commit: line 1 subject, one blank line, then the body.
# A passed message wins. The file counts only when changed since the last commit;
# unchanged, it is still the previous push's message.
$subject = $Message.Trim()
$body = ''
if (-not $subject -and (testBuildNotesChanged) -and (Test-Path -LiteralPath $buildNotes)) {
    $lines = @([IO.File]::ReadAllLines($buildNotes))
    if ($lines.Count -gt 0 -and $lines[0].Trim()) {
        $subject = $lines[0].Trim()
        if ($lines.Count -ge 2 -and $lines[1].Trim()) {
            throw 'buildNotes.txt must have one blank line after the first line, then the commit description.'
        }
        if ($lines.Count -gt 2) { $body = ($lines[2..($lines.Count - 1)] -join "`n").Trim() }
    }
}
if (-not $subject -and -not $DryRun) { $subject = (Read-Host 'Commit message').Trim() }
if (-not $subject) { throw 'Provide a commit message, or write a new one to buildNotes.txt (it is unchanged since the last commit).' }

$branch = ((& git branch --show-current) | Out-String).Trim()
if ($LASTEXITCODE -or -not $branch) { throw 'Could not determine a non-detached current branch.' }
$commitArgs = @('commit', '-m', $subject)
if ($body) { $commitArgs += @('-m', $body) }
$pushArgs = @('push', 'origin', $branch)
if ($Force) { $pushArgs += '--force' }
if ($DryRun) {
    Write-Host 'Dry run: git add -A'
    Write-Host "Dry run: git commit -m '$subject'$(if ($body) { " -m <$($body.Length) chars from buildNotes.txt>" })"
    Write-Host "Dry run: git $($pushArgs -join ' ')"
    return
}
runNativeCommand git @('add', '-A') 'git add'
runNativeCommand git $commitArgs 'git commit'
runNativeCommand git $pushArgs 'git push'
closeOut 3
