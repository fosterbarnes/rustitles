#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [switch]$Clean,
    [string]$WindowsHost = 'foster@goonstation.sawfish-boga.ts.net',
    # Position 0 keeps a bare `x64` or `arm64` from binding to -WindowsHost.
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)][string[]]$BuildArgs
)

$ErrorActionPreference = 'Stop'
$helpText = @'
multiBuild.ps1 [-x64|-arm64] [-Clean] [-WindowsHost <user@host>]
Builds every release asset from any host (macOS, goonstation, win11-arm) and stages them in publish/.
Windows (both archs + installers): this machine if it is Windows, else goonstation, or the SSH host
given with -WindowsHost (e.g. foster@win11-arm.sawfish-boga.ts.net, the Windows 11 ARM VM).
macOS: this machine if it is a Mac, else the MacBook. Linux (amd64): the Debian Parallels VM on the
MacBook (debian-mac), in a podman amd64 container; a run on the MacBook tries to start the VM.
A host that does not answer SSH is skipped with a warning and the rest still build; pushRelease.ps1
needs every asset.
Remote builds get the working tree (tracked + untracked, not ignored) in ~/.rustitles-build/rustitles,
never in a real clone, and their publish/ is copied back with scp.
-Clean: after the assets are copied back, delete the remote ~/.rustitles-build and ~/.rustitles-sync
folders (source, compile cache, upload). The next build on those hosts starts cold. The local
target/ and the Linux container image are kept.
'@
if ($Help) { Write-Host $helpText; return }

. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot

$architecture = getArchitecture (@($BuildArgs) + @($args))
if ($architecture -eq 'help') { Write-Host $helpText; return }

$sshOptions = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', '-o', 'ServerAliveInterval=30')
$probeOptions = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=8')
$macHost = 'foster@macbook.sawfish-boga.ts.net'
$debianHost = 'debianMac@debian-mac.sawfish-boga.ts.net'
$debianVm = 'Debian GNU Linux 13.6 ARM64'
# Relative to the remote user's home on every host.
$syncDir = '.rustitles-sync'
$buildRoot = '.rustitles-build'
$buildDir = "$buildRoot/rustitles"

function encodePwsh([string]$Script) {
    'pwsh -NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
}

function encodeBash([string]$Script) {
    # This file is CRLF, so its here-strings are too; bash needs LF.
    'echo ' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Script.Replace("`r", ''))) + ' | base64 -d | bash'
}

$extractPosix = @"
set -euo pipefail
repo="`$HOME/$buildDir"
mkdir -p "`$repo"
find "`$repo" -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} +
tar -xzf "`$HOME/$syncDir/rustitles.tgz" -C "`$repo"
cd "`$repo"
"@
$macScript = "$extractPosix`nbash .scripts/buildMac.sh`n"
$debianScript = "$extractPosix`nbash .scripts/buildLinux.sh`n"
$windowsArch = if ($architecture) { "'$architecture'" } else { '$null' }
$windowsScript = @"
`$ErrorActionPreference = 'Stop'
`$repo = Join-Path `$HOME '$buildDir'
New-Item -ItemType Directory -Force -Path `$repo | Out-Null
Get-ChildItem -LiteralPath `$repo -Force | Where-Object Name -ne 'target' | Remove-Item -Recurse -Force
tar -xzf (Join-Path `$HOME '$syncDir/rustitles.tgz') -C `$repo
if (`$LASTEXITCODE) { throw "tar failed (exit `$LASTEXITCODE)." }
# Over SSH, .cargo\bin\cargo.exe (a rustup symlink) fails with "untrusted mount point".
`$env:Path = "`$(Split-Path (rustup which cargo));`$env:Path"
`$env:BASE_BUILD_PIPELINE = '1'
. (Join-Path `$repo '.scripts/scriptHelper.ps1')
buildAll -Architecture $windowsArch
"@

$windowsAssets = foreach ($arch in ($architecture ? @($architecture) : @('x64', 'arm64'))) {
    rustitlesWinExeName $arch
    rustitlesWinInstallerName $arch
}
# Each role builds here when this machine can, else on its host over SSH.
$roles = @(
    [pscustomobject]@{ Name = 'Windows'; Local = [bool]$IsWindows; Host = $WindowsHost; Prep = 'pwsh'
        Command = (encodePwsh $windowsScript); Assets = @($windowsAssets) }
    [pscustomobject]@{ Name = 'macOS'; Local = [bool]$IsMacOS; Host = $macHost; Prep = 'posix'
        Command = (encodeBash $macScript); Assets = @((rustitlesMacAppName), (rustitlesMacZipName)) }
    [pscustomobject]@{ Name = 'Debian'; Local = $false; Host = $debianHost; Prep = 'posix'
        Command = (encodeBash $debianScript); Assets = @((rustitlesLinuxAppImageName), (rustitlesLinuxDebName)) }
)

# `exit` is valid in both bash and the PowerShell SSH shell on the Windows hosts.
function testHost([string]$RemoteHost) {
    & ssh @probeOptions $RemoteHost exit *> $null
    -not $LASTEXITCODE
}

function testRoleHost($Role) {
    if (testHost $Role.Host) { return $true }
    # The Debian VM lives on the MacBook, so only a run started there can start it.
    if ($Role.Name -ne 'Debian' -or -not $IsMacOS) { return $false }
    Write-Host 'Debian VM does not answer SSH; trying to start it...'
    & prlctl start $debianVm *> $null
    # Fails for a VM that is already running; one that runs but does not answer is not worth waiting for.
    if ($LASTEXITCODE) { return $false }
    $deadline = (Get-Date).AddSeconds(120)
    do {
        Start-Sleep -Seconds 3
        if (testHost $Role.Host) { return $true }
    } while ((Get-Date) -lt $deadline)
    $false
}

# A host that does not answer is skipped; the other roles still build.
$skipped = @()
$roles = @(foreach ($role in $roles) {
    if ($role.Local -or (testRoleHost $role)) { $role; continue }
    $skipped += $role.Name
    Write-Host "Skipping $($role.Name): $($role.Host) does not answer SSH." -ForegroundColor Yellow
})
if (-not $roles) { throw 'No build host is reachable.' }

# Working tree snapshot for the remote builds.
$archive = Join-Path ([IO.Path]::GetTempPath()) "rustitles-$([Guid]::NewGuid().ToString('N')).tgz"
$fileList = "$archive.files"
$files = @(& git -C $repoRoot ls-files -co --exclude-standard)
if ($LASTEXITCODE) { throw "git ls-files failed (exit $LASTEXITCODE)." }
$files = @($files | Where-Object { Test-Path -LiteralPath (Join-Path $repoRoot $_) -PathType Leaf })
[IO.File]::WriteAllLines($fileList, $files)
# Keeps macOS tar from adding ._* files and extended attributes that GNU tar warns about.
$env:COPYFILE_DISABLE = '1'
runNativeCommand tar @('--no-xattrs', '-czf', $archive, '-C', $repoRoot, '-T', $fileList) 'tar source archive'

$localJob = {
    param($helperPath, $architecture)
    $ErrorActionPreference = 'Stop'
    $PSNativeCommandUseErrorActionPreference = $false
    $env:BASE_BUILD_PIPELINE = '1'
    . $helperPath
    buildAll -Architecture $architecture *>&1 | ForEach-Object { "$_" }
}
$remoteJob = {
    param($sshOptions, $remoteHost, $prepCommand, $archive, $syncDir, $buildCommand)
    $ErrorActionPreference = 'Stop'
    $PSNativeCommandUseErrorActionPreference = $false
    function run([string]$Name, [string]$File, [string[]]$Arguments) {
        # *>&1 keeps native stderr as text instead of CLIXML error records.
        & $File @Arguments *>&1 | ForEach-Object { "$_" }
        if ($LASTEXITCODE) { throw "$Name failed (exit $LASTEXITCODE)." }
    }
    run 'ssh prepare' ssh ($sshOptions + @($remoteHost, $prepCommand))
    run 'scp upload' scp ($sshOptions + @('-q', $archive, "$($remoteHost):$syncDir/rustitles.tgz"))
    run 'ssh build' ssh ($sshOptions + @($remoteHost, $buildCommand))
}
$prepCommands = @{
    pwsh = "New-Item -ItemType Directory -Force -Path $syncDir | Out-Null"
    posix = "mkdir -p $syncDir"
}
$jobColors = @{ Windows = 'Cyan'; macOS = 'Green'; Debian = 'Magenta' }
$startTime = Get-Date

function writeJobOutput($Job) {
    $color = $jobColors[$Job.Name] ?? 'White'
    foreach ($item in @(Receive-Job -Job $Job -ErrorAction Ignore -WarningAction Ignore)) {
        $stamp = '{0:mm\:ss}' -f ((Get-Date) - $startTime)
        foreach ($line in "$item" -split "`n") {
            $trimmed = $line.TrimEnd("`r")
            if (-not $trimmed -or $trimmed -like '*Cannot process the XML*') { continue }
            Write-Host "[$stamp][$($Job.Name)] $trimmed" -ForegroundColor $color
        }
    }
}

$jobs = @()
try {
    foreach ($role in $roles) {
        $jobs += if ($role.Local) {
            Start-Job -Name $role.Name -ScriptBlock $localJob -ArgumentList (Join-Path $PSScriptRoot 'scriptHelper.ps1'), $architecture
        } else {
            Start-Job -Name $role.Name -ScriptBlock $remoteJob -ArgumentList $sshOptions, $role.Host,
                $prepCommands[$role.Prep], $archive, $syncDir, $role.Command
        }
    }
    Write-Host "Building $(($roles | ForEach-Object { "$($_.Name) ($(if ($_.Local) { 'local' } else { $_.Host }))" }) -join ', ')..."
    while (@($jobs | Where-Object State -eq 'Running').Count) {
        foreach ($job in $jobs) { writeJobOutput $job }
        Start-Sleep -Milliseconds 200
    }
    foreach ($job in $jobs) { writeJobOutput $job }

    $failures = @()
    foreach ($job in $jobs) {
        if ($job.State -eq 'Completed') { Write-Host "$($job.Name) build succeeded." -ForegroundColor Green; continue }
        $reason = @($job.ChildJobs | ForEach-Object { $_.JobStateInfo.Reason?.Message } | Where-Object { $_ })
        $failures += "$($job.Name) build failed: $(if ($reason) { $reason -join '; ' } else { $job.State })"
        Write-Host $failures[-1] -ForegroundColor Red
    }
    if ($failures) { throw "Multi-platform build failed:`n$($failures -join "`n")" }
} finally {
    if ($jobs) { Remove-Job -Job $jobs -Force -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $archive, $fileList -Force -ErrorAction SilentlyContinue
}

# Collect remote output after every build finished: the local buildAll clears publish/ when it starts.
New-Item -ItemType Directory -Force -Path $publishFolder | Out-Null
foreach ($role in @($roles | Where-Object { -not $_.Local })) {
    runNativeCommand scp ($sshOptions + @('-q', '-r', "$($role.Host):$buildDir/publish/.", $publishFolder)) "$($role.Name) publish copy"
}

foreach ($name in $roles.Assets) {
    if (-not (Test-Path -LiteralPath (Join-Path $publishFolder $name))) { throw "Missing multi-platform release asset: $name" }
}
# pushRelease.ps1 refuses assets stamped with another version (a bump without a rebuild).
New-Item -ItemType Directory -Force -Path $publishBuildFolder | Out-Null
writeFileNoBom (Join-Path $publishBuildFolder 'version') $versionContents

# Only after every asset is confirmed here: the remote copies are the only other ones.
if ($Clean) {
    $cleanCommands = @{
        pwsh = "Remove-Item -Recurse -Force -LiteralPath $buildRoot, $syncDir"
        posix = "rm -rf $buildRoot $syncDir"
    }
    foreach ($role in @($roles | Where-Object { -not $_.Local })) {
        runNativeCommand ssh ($sshOptions + @($role.Host, $cleanCommands[$role.Prep])) "$($role.Name) cleanup"
        Write-Host "Removed build files on $($role.Host)."
    }
}

Write-Host "Multi-platform build and packaging passed: $($roles.Name -join ', ')."
if ($skipped) {
    Write-Host "Not built (host unreachable): $($skipped -join ', '). pushRelease.ps1 will refuse this publish folder." -ForegroundColor Yellow
}
closeOut 3
