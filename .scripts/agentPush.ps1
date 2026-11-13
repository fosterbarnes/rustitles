#requires -Version 7.0
# Non-interactive push for the agent-push skill: -Context prints what the agent needs to write
# buildNotes.txt; the default mode validates it, runs prePush.ps1 and push.ps1 quietly, then reports.
# -Tags prints 10 candidate version tags; -SetTag writes the one the user picked.
# -DryRun runs every check and the real prePush.ps1, then push.ps1 -DryRun; nothing is committed.
# The user picks the version bump: -Context prints an ASK line; -Bump applies the answer, and -NoBump
# lets a push with code changes keep the current version.
param(
    [switch]$Context, [switch]$Tags, [string]$SetTag,
    [ValidateSet('major', 'minor', 'patch')][string]$Bump, [switch]$NoBump, [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot
$current = requireVersion
$headVersion = getHeadVersion
if ($Tags) { getVersionTags 10; return }
if ($SetTag) {
    $set = setVersionTag $SetTag
    Write-Host "line 1: v$current $set [<label> > <summary>]"
    return
}
if ($Bump) {
    if ($current -ne $headVersion) { throw "Already bumped since the last commit ($headVersion -> $current)." }
    $next = bumpVersionPart $Bump
    Write-Host "line 1: v$next $(requireVersionTag) [<label> > <summary>]"
    return
}
$tagWord = requireVersionTag
$notesChanged = testBuildNotesChanged

$unbumped = getUnbumpedCodeChanges
$bumpFix = "Code changed but the version is still $current and no bump was chosen. Ask the user, then run " +
    'agentPush.ps1 -Bump patch|minor|major (and update the buildNotes.md heading and Version bullet), ' +
    'or push with agentPush.ps1 -NoBump.'

if ($Context) {
    Write-Host "branch: $(git branch --show-current)  version: $current $tagWord"
    Write-Host "line 1: v$current $tagWord [<label> > <summary>]"
    Write-Host "labels (decision order; rules in AGENTS.md 'buildNotes.txt' section): $($commitLabels -join ' ')"
    Write-Host 'release: off (run multiBuild.ps1 and pushRelease.ps1 yourself)'
    Write-Host '--- status'
    git status --short 2>$null
    # buildNotes.md is gitignored here, so show its newest entry instead of a diff.
    Write-Host '--- newest buildNotes.md entry'
    $notesMd = Join-Path $repoRoot 'buildNotes.md'
    if (Test-Path -LiteralPath $notesMd) {
        $entry = [regex]::Match([IO.File]::ReadAllText($notesMd), '(?ms)^## \d{4}-.*?(?=^---\s*$|\z)')
        if ($entry.Success) { $entry.Value.TrimEnd() }
    }
    Write-Host '--- last 10 subjects'
    git log -10 --format=%s
    Write-Host '--- last commit message (style sample)'
    git log -1 --format=%B
    if ($notesChanged) { Write-Host 'WARN: buildNotes.txt already changed since HEAD; overwrite it.' }
    if ($current -ne $headVersion) {
        Write-Host "version: bumped since the last commit ($headVersion -> $current)"
        return
    }
    # Patch is the usual answer for code changes; docs and notes alone usually keep the version.
    $choices = @(
        "patch ($(getNextVersion patch))", "minor ($(getNextVersion minor))", "major ($(getNextVersion major))"
    )
    $choices = if ($unbumped) { $choices + "none (keep $current)" } else { @("none (keep $current)") + $choices }
    Write-Host ("ASK: Bump the version for this push? Options, recommended first: $($choices -join ', '). " +
        'Apply major/minor/patch with agentPush.ps1 -Bump <part> before writing buildNotes.txt; for none, ' +
        "push with agentPush.ps1 -NoBump$(if (-not $unbumped) { ' (no code changed, so -NoBump is optional)' }).")
    return
}

# Fail on a bad message before the slow checks run; -DryRun collects every problem and keeps going.
$problems = [Collections.Generic.List[string]]::new()
function check([scriptblock]$Test) {
    try { & $Test } catch { if (-not $DryRun) { throw }; $problems.Add("$_") }
}
check { if ($unbumped -and -not $NoBump) { throw "$bumpFix Changed: $($unbumped -join ', ')" } }
check { if (-not $notesChanged) { throw 'buildNotes.txt is unchanged since the last commit; write the new message first.' } }
check {
    $lines = @([IO.File]::ReadAllLines($buildNotes))
    assertCommitSubject $lines[0].Trim() "v$current $tagWord"
    if ($lines.Count -lt 3 -or $lines[1].Trim() -or -not ($lines[2..($lines.Count - 1)] -join '').Trim()) {
        throw 'buildNotes.txt needs line 1, one blank line, then a non-empty body.'
    }
}

# Run each script in-process (closeOut is a no-op when not the -File entry); print the tail only on failure.
foreach ($step in 'prePush', 'push') {
    $stepArgs = @{}
    if ($DryRun -and $step -eq 'push') { $stepArgs.DryRun = $true }
    $log = [Collections.Generic.List[string]]::new()
    try { & (Join-Path $PSScriptRoot "$step.ps1") @stepArgs *>&1 | ForEach-Object { $log.Add("$_") } }
    catch {
        $log | Select-Object -Last 40
        throw "$step.ps1 failed: $_"
    }
    if ($stepArgs.Count) { $log; continue }
    Write-Host "$step.ps1 passed."
    # multiBuild skips unreachable hosts without failing; keep that visible.
    $log | Where-Object { $_ -match '^(Skipping \w+:|Not built \()' }
}

if ($DryRun) {
    Write-Host '--- dry run: nothing was committed or pushed'
    if ($problems) { throw "agentPush would stop before prePush:`n- $($problems -join "`n- ")" }
    Write-Host 'agentPush would pass.'
    return
}

Set-Location -LiteralPath $repoRoot
Write-Host "--- pushed $(git rev-parse --short=7 HEAD) to origin/$(git branch --show-current)"
git show -s --format=%B HEAD
git show --name-status --format= HEAD
