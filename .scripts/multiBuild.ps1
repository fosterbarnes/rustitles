#requires -Version 7.0
param(
    [Alias('h')][switch]$Help,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$BuildArgs
)

$ErrorActionPreference = 'Stop'
$helpText = @'
multiBuild.ps1 [-x64|-arm64]
Runs on Windows and requires SSH targets plus Windows-visible copy roots for macOS, Windows ARM, and Debian.
Copies source to each host, builds all configured platforms concurrently, and stages final assets in the local publish folder.
'@
if ($Help) { Write-Host $helpText; return }

.(Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot

$architecture = getArchitecture (@($BuildArgs) + @($args))
if ($architecture -eq 'help') { Write-Host $helpText; return }
if (-not $IsWindows) { throw 'multiBuild.ps1 must run on Windows because its copy operators use Robocopy.' }

$projectFolder = Split-Path -Path $repoRoot -Leaf
$macHost = $env:SSH_MAC_TARGET
$macRoot = $env:GITHUB_MAC_REMOTE_ROOT
$macRepo = if ($macRoot) { "$($macRoot.TrimEnd('/'))/$projectFolder" } else { $null }
$windowsArmHost = $env:SSH_WIN11_ARM_TARGET
$windowsArmRoot = $env:GITHUB_WIN11_ARM_REMOTE_ROOT
$windowsArmRepo = if ($windowsArmRoot) { Join-Path $windowsArmRoot $projectFolder } else { $null }
$debianHost = $env:SSH_DEBIAN_TARGET
$debianRoot = $env:GITHUB_DEBIAN_REMOTE_ROOT
$debianCopyRoot = $env:GITHUB_DEBIAN_COPY_ROOT
$debianRepo = if ($debianRoot) { "$($debianRoot.TrimEnd('/'))/$projectFolder" } else { $null }
if (-not $macHost -or -not $windowsArmHost -or -not $debianHost -or
    -not $macRepo -or -not $windowsArmRepo -or -not $debianCopyRoot -or -not $debianRepo) {
    throw 'SSH targets and remote GitHub roots are not configured. Load the user profile before running multiBuild.ps1.'
}

function startBuildJob {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][scriptblock]$ScriptBlock,
        [object[]]$ArgumentList
    )
    Start-Job -Name $Name -ScriptBlock $ScriptBlock -ArgumentList @($ArgumentList)
}

function pullRemotePublish {
    param(
        [Parameter(Mandatory)][string]$RemoteHost,
        [Parameter(Mandatory)][string]$RemotePublish,
        [Parameter(Mandatory)][string]$Name
    )
    $source = "$($RemoteHost):$RemotePublish/."
    runNativeCommand scp @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', '-r', $source, $publishFolder) $Name
}

function quotePosixLiteral {
    param([Parameter(Mandatory)][string]$Value)
    return "'" + $Value.Replace("'", "'\''") + "'"
}

function quotePowerShellLiteral {
    param([Parameter(Mandatory)][string]$Value)
    return "'" + $Value.Replace("'", "''") + "'"
}

function ensureDebianVmReady {
    $vmxPath = 'C:\VMware VMs\Debian 13 KDE\Debian 13 KDE.vmx'
    $vmrunPath = (Get-Command vmrun.exe -ErrorAction SilentlyContinue)?.Source
    if (-not $vmrunPath) {
        foreach ($candidate in @(
            'C:\Program Files (x86)\VMware\VMware Workstation\vmrun.exe'
            'C:\Program Files\VMware\VMware Workstation\vmrun.exe'
        )) {
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $vmrunPath = $candidate
                break
            }
        }
    }
    if (-not $vmrunPath) { throw 'VMware vmrun.exe was not found.' }
    if (-not (Test-Path -LiteralPath $vmxPath -PathType Leaf)) { throw "Debian VMX was not found: $vmxPath" }

    $runningVms = & $vmrunPath '-T' 'ws' 'list' 2>&1
    if ($LASTEXITCODE) { throw "vmrun list failed (exit $LASTEXITCODE)." }
    $isRunning = @($runningVms | ForEach-Object { "$($_)".Trim() } |
        Where-Object { [string]::Equals($_, $vmxPath, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
    if (-not $isRunning) {
        Write-Host 'Starting Debian VMware VM...'
        runNativeCommand $vmrunPath @('-T', 'ws', 'start', $vmxPath, 'nogui') 'Debian VM start'
    } else {
        Write-Host 'Debian VMware VM is already running.'
    }

    Write-Host 'Waiting for Debian SSH...'
    $deadline = (Get-Date).AddSeconds(180)
    do {
        & ssh @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=5', $debianHost, 'true') 1>$null 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Host 'Debian SSH is ready.'
            return
        }
        if ((Get-Date) -ge $deadline) { throw "Debian SSH did not become ready within 180 seconds: $debianHost" }
        Start-Sleep -Seconds 3
    } while ($true)
}

function assertMultiBuildArtifacts {
    param([string]$Architecture)
    $architectures = if ($Architecture) { @($Architecture) } else { @('x64', 'arm64') }
    $expected = @(
        foreach ($arch in $architectures) { rustitlesWinExeName $arch; rustitlesWinInstallerName $arch }
        (rustitlesMacAppName)
        (rustitlesMacZipName)
        (rustitlesLinuxAppImageName)
        (rustitlesLinuxDebName)
    )
    foreach ($name in $expected) {
        $path = Join-Path $publishFolder $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -and -not (Test-Path -LiteralPath $path -PathType Container)) {
            throw "Missing multi-platform release asset: $path"
        }
    }
}

$previousPipeline = $env:BASE_BUILD_PIPELINE
try {
    $env:BASE_BUILD_PIPELINE = '1'

    Write-Host 'Copying source to macOS host...'
    runNativeCommand pwsh @(
        '-NoLogo'
        '-NoProfile'
        '-File'
        (Join-Path $PSScriptRoot 'copyTo.ps1')
        '-Mac'
    ) 'copyTo.ps1 -Mac'

    Write-Host 'Copying source to Windows ARM host...'
    runNativeCommand pwsh @(
        '-NoLogo'
        '-NoProfile'
        '-File'
        (Join-Path $PSScriptRoot 'copyTo.ps1')
        '-Win11Arm'
    ) 'copyTo.ps1 -Win11Arm'

    ensureDebianVmReady
    Write-Host 'Copying source to Debian host...'
    runNativeCommand pwsh @(
        '-NoLogo'
        '-NoProfile'
        '-File'
        (Join-Path $PSScriptRoot 'copyTo.ps1')
        '-Debian'
    ) 'copyTo.ps1 -Debian'

    $macRepoLiteral = quotePosixLiteral $macRepo
    $macSteps = @(
        "cd $macRepoLiteral"
        'rm -rf publish'
        'bash .scripts/buildMac.sh'
    )
    $remoteHelper = "$windowsArmRepo\.scripts\scriptHelper.ps1"
    $remoteArchitecture = 'arm64'
    $remoteHelperLiteral = quotePowerShellLiteral $remoteHelper
    $remoteArchitectureLiteral = quotePowerShellLiteral $remoteArchitecture
    $remoteScript = @(
        '$cargoDir = Split-Path (rustup which cargo)'
        '$env:Path = "$cargoDir;$env:Path"'
        ". $remoteHelperLiteral"
        "buildAll -Architecture $remoteArchitectureLiteral"
    ) -join "`n"
    $encodedRemoteScript = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($remoteScript))
    $remoteCommand = "pwsh -NoLogo -NoProfile -NonInteractive -EncodedCommand $encodedRemoteScript"
    $debianRepoLiteral = quotePosixLiteral $debianRepo
    $debianSteps = @(
        "cd $debianRepoLiteral"
        'rm -rf publish'
        'bash .scripts/buildDebian.sh'
    )
    $buildJobs = @()
    $buildResults = @()
    $localBuildScript = {
        param($helperPath, $localRepo, $localArchitecture)
        $ErrorActionPreference = 'Stop'
        $PSNativeCommandUseErrorActionPreference = $false
        . $helperPath
        $env:BASE_BUILD_PIPELINE = '1'
        Set-Location -LiteralPath $localRepo
        buildAll -Architecture $localArchitecture *>&1 | ForEach-Object { "$_" }
    }
    $remoteBuildScript = {
        param($remoteHost, $remoteCommand, $buildName)
        $ErrorActionPreference = 'Stop'
        $PSNativeCommandUseErrorActionPreference = $false
        # Use *>&1 to merge all streams and avoid CLIXML error-stream serialization for native ssh stderr
        & ssh @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', $remoteHost, $remoteCommand) *>&1 | ForEach-Object { "$_" }
        $exitCode = $LASTEXITCODE
        if ($exitCode) { throw "$buildName failed (exit $exitCode)." }
    }

    try {
        $buildJobs += startBuildJob 'Windows' $localBuildScript @(
            (Join-Path $PSScriptRoot 'scriptHelper.ps1')
            $repoRoot
            $architecture
        )
        $buildJobs += startBuildJob 'macOS' $remoteBuildScript @($macHost, ($macSteps -join ' && '), 'macOS build')
        $buildJobs += startBuildJob 'Windows ARM' $remoteBuildScript @($windowsArmHost, $remoteCommand, 'Windows ARM build')
        $buildJobs += startBuildJob 'Debian' $remoteBuildScript @($debianHost, ($debianSteps -join ' && '), 'Debian build')

        Write-Host 'Building all platforms concurrently (live output, color per host)...'
        $jobColors = @{
            'Windows'      = 'Cyan'
            'macOS'        = 'Green'
            'Windows ARM'  = 'Yellow'
            'Debian'       = 'Magenta'
        }
        # Live stream: poll jobs and print new output with color prefix and timestamp
        $startTime = Get-Date
        do {
            $anyRunning = $false
            foreach ($job in $buildJobs) {
                $color = $jobColors[$job.Name]
                if (-not $color) { $color = 'White' }
                # Drain available output without blocking; Receive-Job consumes so next poll gets only new data
                # Use -ErrorAction Ignore to suppress CLIXML deserialization spam from native stderr (now merged via *>&1)
                $received = @(Receive-Job -Job $job -ErrorAction Ignore -WarningAction Ignore)
                foreach ($item in @($received)) {
                    $text = "$item"
                    if ($text.Length -eq 0) { continue }
                    # Suppress the XML deserialization noise if it still appears as text
                    if ($text -like "*Cannot process the XML*") { continue }
                    $elapsed = (Get-Date) - $startTime
                    $stamp = '{0:mm\:ss}' -f $elapsed
                    foreach ($line in $text -split "`n") {
                        $trimmed = $line.TrimEnd("`r")
                        if ($trimmed.Length -eq 0) { continue }
                        if ($trimmed -like "*Cannot process the XML*") { continue }
                        Write-Host "[$stamp][$($job.Name)] $trimmed" -ForegroundColor $color
                    }
                }
                if ($job.State -eq 'Running') { $anyRunning = $true }
            }
            if ($anyRunning) { Start-Sleep -Milliseconds 200 }
        } while ($anyRunning)
        # Final drain for any remaining buffered output on completed jobs (in case last poll missed)
        foreach ($job in $buildJobs) {
            $color = $jobColors[$job.Name]
            if (-not $color) { $color = 'White' }
            $received = @(Receive-Job -Job $job -ErrorAction Ignore -WarningAction Ignore)
            foreach ($item in @($received)) {
                $text = "$item"
                if ($text.Length -eq 0) { continue }
                if ($text -like "*Cannot process the XML*") { continue }
                $elapsed = (Get-Date) - $startTime
                $stamp = '{0:mm\:ss}' -f $elapsed
                foreach ($line in $text -split "`n") {
                    $trimmed = $line.TrimEnd("`r")
                    if ($trimmed.Length -eq 0) { continue }
                    if ($trimmed -like "*Cannot process the XML*") { continue }
                    Write-Host "[$stamp][$($job.Name)] $trimmed" -ForegroundColor $color
                }
            }
        }
        foreach ($job in $buildJobs) {
            $reason = @($job.ChildJobs | ForEach-Object { $_.JobStateInfo.Reason } |
                Where-Object { $_ } | ForEach-Object { $_.Exception.Message })
            $success = $job.State -eq 'Completed'
            $buildResults += [pscustomobject]@{
                Name = $job.Name
                Success = $success
                Detail = if ($success) { 'completed' } else {
                    if ($reason.Count -gt 0) { $reason -join '; ' } else { $job.State }
                }
            }
            $status = if ($success) { 'succeeded' } else { 'failed' }
            $statusColor = if ($success) { 'Green' } else { 'Red' }
            Write-Host "$($job.Name) build $status." -ForegroundColor $statusColor
        }
    } finally {
        if ($buildJobs.Count -gt 0) { Remove-Job -Job $buildJobs -Force -ErrorAction SilentlyContinue }
    }

    $pullFailures = @()
    foreach ($remoteBuild in @(
        @{ Name = 'macOS'; Host = $macHost; Publish = "$macRepo/publish" }
        @{ Name = 'Windows ARM'; Host = $windowsArmHost; Publish = (($windowsArmRepo -replace '\\', '/') + '/publish') }
        @{ Name = 'Debian'; Host = $debianHost; Publish = "$debianRepo/publish" }
    )) {
        $result = $buildResults | Where-Object Name -eq $remoteBuild.Name
        if (-not $result.Success) { continue }
        try {
            pullRemotePublish $remoteBuild.Host $remoteBuild.Publish "$($remoteBuild.Name) publish"
        } catch {
            $pullFailures += "$($remoteBuild.Name) publish failed: $($_.Exception.Message)"
        }
    }

    $buildFailures = @($buildResults | Where-Object { -not $_.Success } |
        ForEach-Object { "$($_.Name) build failed: $($_.Detail)" })
    $failures = @($buildFailures) + @($pullFailures)
    if ($failures.Count -gt 0) {
        throw "Multi-platform build failed:`n$($failures -join "`n")"
    }
    assertMultiBuildArtifacts -Architecture $architecture
} finally { $env:BASE_BUILD_PIPELINE = $previousPipeline }

Write-Host 'Multi-platform build and packaging passed.'
closeOut 3
