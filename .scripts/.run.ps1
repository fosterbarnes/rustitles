#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$AppLaunchArgs
)
$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host '.run.ps1 [-x64|-arm64] [-- app args...]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')

$architectureArgs = @($AppLaunchArgs | Where-Object { "$_" -match '(?i)^(--x64|-x64|--64|-64|--arm64|-arm64|--arm|-arm|--help|-h)$' })
$architecture = if ($architectureArgs) { getArchitecture $architectureArgs } else { $null }
if ($architecture -eq 'help') { Write-Host '.run.ps1 [-x64|-arm64] [-- app args...]'; return }
$forward = @($AppLaunchArgs | Where-Object { "$_" -notmatch '(?i)^(--x64|-x64|--64|-64|--arm64|-arm64|--arm|-arm)$' })
Set-Location -LiteralPath $repoRoot

function getOwnedProcessIds {
    param([Parameter(Mandatory)][int]$RootId)

    $parents = @{}
    if ($IsWindows) {
        foreach ($process in @(Get-CimInstance Win32_Process -ErrorAction Stop)) {
            $parents[[int]$process.ProcessId] = [int]$process.ParentProcessId
        }
    } else {
        $psCommand = (Get-Command ps -CommandType Application -ErrorAction Stop).Source
        foreach ($line in @(& $psCommand -eo 'pid=,ppid=' 2>$null)) {
            if ($line -match '^\s*(\d+)\s+(\d+)\s*$') {
                $parents[[int]$Matches[1]] = [int]$Matches[2]
            }
        }
    }

    $owned = [System.Collections.Generic.List[int]]::new()
    $owned.Add($RootId)
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($entry in $parents.GetEnumerator()) {
            if ($owned.Contains([int]$entry.Value) -and -not $owned.Contains([int]$entry.Key)) {
                $owned.Add([int]$entry.Key)
                $changed = $true
            }
        }
    }
    @($owned)
}

function stopOwnedProcessTree {
    param([Parameter(Mandatory)][int]$RootId)

    $owned = getOwnedProcessIds $RootId
    foreach ($processId in $owned) {
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ([DateTime]::UtcNow -lt $deadline) {
        $running = @($owned | Where-Object {
            Get-Process -Id $_ -ErrorAction SilentlyContinue
        })
        if (-not $running) { return }
        Start-Sleep -Milliseconds 25
    }
    throw "Owned cargo process tree did not exit cleanly."
}

while ($true) {
    $cargoArgs = @('run', '--manifest-path', $manifest)
    if ($IsWindows -and $architecture) { $cargoArgs += @('--target', (getWindowsCargoTarget $architecture)) }
    if ($forward.Count -gt 0) { $cargoArgs += @('--') + [string[]]$forward }
    $proc = Start-Process -FilePath 'cargo' -ArgumentList $cargoArgs -WorkingDirectory $repoRoot -NoNewWindow -PassThru
    Write-Host "=== running $projectName... === `nq = quit `nr , up arrow = restart"
    $action = $null
    while (-not $proc.HasExited) {
        Start-Sleep -Milliseconds 50
        try { if (-not [Console]::KeyAvailable) { continue } } catch { continue }
        $key = [Console]::ReadKey($true)
        if ($key.Key -eq [ConsoleKey]::R -or $key.Key -eq [ConsoleKey]::UpArrow) { $action = 'restart'; break }
        if ($key.Key -eq [ConsoleKey]::Q) { $action = 'quit'; break }
    }
    if ($action) {
        stopOwnedProcessTree $proc.Id
        $proc.WaitForExit()
    } else {
        $exitCode = $proc.ExitCode
        if ($exitCode -ne 0) { throw "cargo run failed (exit $exitCode)." }
    }
    if ($action -ne 'restart') { break }
}
closeOut 3
