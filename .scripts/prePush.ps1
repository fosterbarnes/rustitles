#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$BuildArgs
)
$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host 'prePush.ps1 [-x64|-arm64]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
# Fast gate: tests + clippy before heavy multi-platform build
runNativeCommand cargo @('test', '--workspace', '--quiet') 'cargo test'
runNativeCommand cargo @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') 'cargo clippy'
runNativeCommand cargo @('fmt', '--check') 'cargo fmt'
$multiBuildArgs = @($BuildArgs) + @($args)
$previousPipeline = $env:BASE_BUILD_PIPELINE
try {
    $env:BASE_BUILD_PIPELINE = '1'
    & (Join-Path $PSScriptRoot 'multiBuild.ps1') @multiBuildArgs
} finally {
    $env:BASE_BUILD_PIPELINE = $previousPipeline
}
Write-Host 'Pre-push checks passed (test + clippy + fmt + multi-build). No commit or push was performed.'
closeOut 3
