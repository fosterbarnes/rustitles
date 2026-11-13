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
Every bump also rolls a new version tag (.version/versionTag). For patch-only bumps use bumpVersion.ps1.
"@
}

$bump = $null
$dir = 'up'
foreach ($flag in @($args | ForEach-Object { "$_".Trim() } | Where-Object Length)) {
    switch -Regex ($flag) {
        '(?i)^(-h|--help)$' { showHelp; return }
        '^\+{1,3}$' { if ($bump) { throw 'Use only one bump flag.' }; $bump = @{1 = 'major'; 2 = 'minor'; 3 = 'patch'}[$flag.Length]; $dir = 'up' }
        '^\-\+{1,3}$' { if ($bump) { throw 'Use only one bump flag.' }; $bump = @{1 = 'major'; 2 = 'minor'; 3 = 'patch'}[$flag.TrimStart('-').Length]; $dir = 'up' }
        '^\-{1,3}$' { if ($bump) { throw 'Use only one bump flag.' }; $bump = @{1 = 'major'; 2 = 'minor'; 3 = 'patch'}[$flag.Length]; $dir = 'down' }
        default { throw "Unknown argument: $flag" }
    }
}
if (-not $bump) { $bump = 'patch' }
$current = requireVersion
$parts = @($current -split '\.' | ForEach-Object { [int]$_ })
$major, $minor, $patch = $parts
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
$newVersion = "$major.$minor.$patch"
setVersion $newVersion
$newTag = newVersionTag
Write-Host "Version $current -> $newVersion ($newTag)"
closeOut 3
