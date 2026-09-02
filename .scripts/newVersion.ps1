#requires -Version 7.0
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot

function showHelp {
    Write-Host @"
newVersion.ps1              Patch bump
newVersion.ps1 -+ / +       Major bump
newVersion.ps1 -++ / ++     Minor bump
newVersion.ps1 -+++ / +++   Patch bump
newVersion.ps1 -            Major bump down
newVersion.ps1 --           Minor bump down
newVersion.ps1 ---          Patch bump down
newVersion.ps1 -tag         Set the optional release tag
"@
}

$wantTag = $false
$bump = $null
$dir = 'up'
foreach ($flag in @($args | ForEach-Object { "$_".Trim() } | Where-Object Length)) {
    switch -Regex ($flag) {
        '(?i)^(-h|--help)$' { showHelp; return }
        '(?i)^(-tag|--tag)$' { $wantTag = $true }
        '^\+{1,3}$' { if ($bump) { throw 'Use only one bump flag.' }; $bump = @{1 = 'major'; 2 = 'minor'; 3 = 'patch'}[$flag.Length]; $dir = 'up' }
        '^\-\+{1,3}$' { if ($bump) { throw 'Use only one bump flag.' }; $bump = @{1 = 'major'; 2 = 'minor'; 3 = 'patch'}[$flag.TrimStart('-').Length]; $dir = 'up' }
        '^\-{1,3}$' { if ($bump) { throw 'Use only one bump flag.' }; $bump = @{1 = 'major'; 2 = 'minor'; 3 = 'patch'}[$flag.Length]; $dir = 'down' }
        default { throw "Unknown argument: $flag" }
    }
}
if (-not $bump -and -not $wantTag) { $bump = 'patch'; $dir = 'up' }
$lines = readVerFile
$parts = @($lines[0] -split '\.')
if ($parts.Count -ne 3 -or (@($parts | Where-Object { $_ -notmatch '^\d+$' })).Count -gt 0) { throw "Invalid semantic version: $($lines[0])" }
$major = [int]$parts[0]; $minor = [int]$parts[1]; $patch = [int]$parts[2]
if ($dir -eq 'down') {
    switch ($bump) {
        'major' { if ($major -gt 0) { $major--; $minor = 0; $patch = 0 } }
        'minor' { if ($minor -gt 0) { $minor--; $patch = 0 } }
        'patch' { if ($patch -gt 0) { $patch-- } }
    }
} else {
    switch ($bump) {
        'major' { $major++; $minor = 0; $patch = 0 }
        'minor' { $minor++; $patch = 0 }
        'patch' { $patch++ }
    }
}
$tagValue = $lines[1]
if ($wantTag) { $tagValue = Read-Host 'Release tag (leave empty to use v<version>)' }
$newVersion = "$major.$minor.$patch"
writeVerFile -SemVer $newVersion -Tag $tagValue -Build $lines[2]
syncVersionConsumers $newVersion
if (Test-Path -LiteralPath $buildNotes) {
    $first = ([IO.File]::ReadAllLines($buildNotes) | Select-Object -First 1)
    if ($first -match '^v\d') {
        $tail = @([IO.File]::ReadAllLines($buildNotes) | Select-Object -Skip 1)
        writeFileNoBom -LiteralPath $buildNotes -Content ((@("v$newVersion release") + $tail) -join "`n")
    }
}
Write-Host "Version -> $newVersion"
closeOut 3
