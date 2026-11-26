#requires -Version 7.0
# Patch bump plus a new version tag (the agent-push and build-notes skills call this), or -RerollTag
# for a new tag on the same version.
# CmdletBinding: an unknown flag must fail, not be ignored and bump the version.
[CmdletBinding()]
param([Alias('h')][switch]$Help, [switch]$RerollTag)
$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host 'bumpVersion.ps1 [-RerollTag]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')

if ($RerollTag) {
    Write-Host "Version tag $(newVersionTag)"
} else {
    bumpVersionPart patch | Out-Null
}
