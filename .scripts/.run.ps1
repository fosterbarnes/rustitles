#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$AppLaunchArgs
)
$ErrorActionPreference = 'Stop'
if ($Help) { Write-Host '.run.ps1 [-x64|-arm64] [-- app args...]'; return }
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')

function stopCargoApp {
    param([Parameter(Mandatory)]$Process)
    if (-not $Process.HasExited) {
        try { $Process.Kill() } catch { }
    }
    Stop-Process -Name 'rustitles' -Force -ErrorAction SilentlyContinue
}

$architectureArgs = @($AppLaunchArgs | Where-Object { "$_" -match '(?i)^(--x64|-x64|--64|-64|--arm64|-arm64|--arm|-arm|--help|-h)$' })
$architecture = if ($architectureArgs) { getArchitecture $architectureArgs } else { $null }
if ($architecture -eq 'help') { Write-Host '.run.ps1 [-x64|-arm64] [-- app args...]'; return }
$forward = @($AppLaunchArgs | Where-Object { "$_" -notmatch '(?i)^(--x64|-x64|--64|-64|--arm64|-arm64|--arm|-arm)$' })
Set-Location -LiteralPath $repoRoot

$keepRunning = $true
while ($keepRunning) {
    $cargoArgs = @('run', '--manifest-path', $manifest)
    if ($IsWindows -and $architecture) { $cargoArgs += @('--target', (getWindowsCargoTarget $architecture)) }
    if ($forward.Count -gt 0) { $cargoArgs += @('--') + [string[]]$forward }
    $proc = Start-Process -FilePath 'cargo' -ArgumentList $cargoArgs -WorkingDirectory $repoRoot -NoNewWindow -PassThru
    Write-Host "=== running $projectName... === `nq = quit `nr , up arrow = restart"
    $restartRequested = $false; $lineBuffer = ''
    while (-not $proc.HasExited) {
        Start-Sleep -Milliseconds 50
        try { if (-not [Console]::KeyAvailable) { continue } } catch { continue }
        $key = [Console]::ReadKey($true)
        if ($key.Key -eq [ConsoleKey]::UpArrow) { stopCargoApp -Process $proc; $restartRequested = $true; break }
        if ($key.Key -eq [ConsoleKey]::Enter) {
            Write-Host ''; $userInput = $lineBuffer.Trim(); $lineBuffer = ''
            if ($userInput -in @('q', 'quit', 'exit')) {
                stopCargoApp -Process $proc
                $keepRunning = $false; break
            }
            if ($userInput -in @('r', 'restart')) { stopCargoApp -Process $proc; $restartRequested = $true; break }
            continue
        }
        if ($key.Key -eq [ConsoleKey]::Backspace) {
            if ($lineBuffer.Length -gt 0) { $lineBuffer = $lineBuffer.Substring(0, $lineBuffer.Length - 1); Write-Host "`b `b" -NoNewline }
            continue
        }
        if ($key.KeyChar -and -not [char]::IsControl($key.KeyChar)) { $lineBuffer += $key.KeyChar; Write-Host -NoNewline $key.KeyChar }
    }
    if ($proc.HasExited) {
        Write-Host 'App stopped.'
        try { $code = $proc.ExitCode; if ($null -ne $code -and $code -ne 0) { Write-Host "cargo exit code: $code" -ForegroundColor Red } } catch { }
    }
    if (-not $keepRunning -or -not $restartRequested) { break }
}
closeOut 3
