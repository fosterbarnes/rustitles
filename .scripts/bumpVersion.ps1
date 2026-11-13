#requires -Version 7.0
# Patch bump plus a new version tag (the agent-push and build-notes skills call this), or -RerollTag
# for a new tag on the same version.
param([switch]$RerollTag)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')

if ($RerollTag) {
    Write-Host "Version tag $(newVersionTag)"
} else {
    bumpVersionPart patch | Out-Null
}
