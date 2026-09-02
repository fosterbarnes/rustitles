#requires -Version 7.0
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Path $PSScriptRoot -Parent
$projectName = 'rustitles'
$crateRoot = Join-Path $repoRoot 'crate'
$manifest = Join-Path $repoRoot 'Cargo.toml'
$crateManifest = Join-Path $crateRoot 'Cargo.toml'
$versionTxt = Join-Path $crateRoot 'src/version.txt'
$configFile = Join-Path $crateRoot 'src/config.rs'
$versionFolder = Join-Path $repoRoot '.version'
$version = Join-Path $versionFolder 'version'
$versionBuild = Join-Path $versionFolder 'versionBuild'
$versionTag = Join-Path $versionFolder 'versionTag'
$buildNotes = Join-Path $repoRoot 'buildNotes.txt'
$readme = Join-Path $repoRoot 'README.md'
$installerFolder = Join-Path $repoRoot '.installer'
$installerOutput = Join-Path $installerFolder 'Output'
$publishFolder = Join-Path $repoRoot 'publish'
$publishBuildFolder = Join-Path $publishFolder 'build'
$appPublisher = 'fosterbarnes'
$appURL = "https://github.com/$appPublisher/$projectName"
$ghRepo = "$appPublisher/$projectName"
$versionLines = @([IO.File]::ReadAllText($version) -split "\r?\n")
while ($versionLines.Count -lt 3) { $versionLines += '' }
$versionContents = $versionLines[0]
$versionTagContents = if (Test-Path -LiteralPath $versionTag) { ([IO.File]::ReadAllText($versionTag)).Trim() } else { $versionLines[1] }
$tag = if ($versionTagContents) { $versionTagContents } else { "v$versionContents" }
$noBom = [Text.UTF8Encoding]::new($false)
$weztermExe = (Get-Command wezterm.exe -ErrorAction SilentlyContinue)?.Source
$windowsInstallerTargets = @(
    @{
        Architecture = 'x64'
        BinFolder = Join-Path $publishBuildFolder 'x64'
        ExePath = Join-Path (Join-Path $publishBuildFolder 'x64') 'rustitles.exe'
        InstallerName = 'rustitles-x64-installer.exe'
        InstallerScript = Join-Path $installerFolder 'rustitles.x64.installer.iss'
    }
    @{
        Architecture = 'arm64'
        BinFolder = Join-Path $publishBuildFolder 'arm64'
        ExePath = Join-Path (Join-Path $publishBuildFolder 'arm64') 'rustitles.exe'
        InstallerName = 'rustitles-arm64-installer.exe'
        InstallerScript = Join-Path $installerFolder 'rustitles.arm64.installer.iss'
    }
)

function writeClearedLine {
    param([Parameter(Mandatory)][string]$Text, [Parameter(Mandatory)][int]$PadWidth, [switch]$NoNewline)
    Write-Host "`r$Text$(' ' * [Math]::Max(0, $PadWidth - $Text.Length))" -NoNewline:$NoNewline
}

function closeOut {
    param([int]$Seconds = 5)
    if ($env:BASE_BUILD_PIPELINE) { return }
    if ($Seconds -lt 0) { $Seconds = 0 }
    if ($Seconds -gt 0) {
        $pad = "closing after $Seconds seconds..."
        foreach ($n in $Seconds..1) {
            writeClearedLine -Text "closing after $n seconds..." -PadWidth $pad.Length -NoNewline
            Start-Sleep -Seconds 1
        }
        writeClearedLine -Text 'closing...' -PadWidth $pad.Length
    }
    $caller = $MyInvocation.PSCommandPath
    if ([string]::IsNullOrWhiteSpace($caller)) { return }
    $argv = [Environment]::GetCommandLineArgs()
    $fileArg = $null
    for ($i = 0; $i -lt $argv.Length; $i++) {
        if ("$($argv[$i])" -match '^(?i)-File$|^(?i)-f$') {
            if ($i + 1 -lt $argv.Length) { $fileArg = $argv[$i + 1] }
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($fileArg)) { return }
    try {
        $fileFull = [IO.Path]::GetFullPath($fileArg)
        $callerFull = [IO.Path]::GetFullPath($caller)
    } catch { return }
    if (-not [string]::Equals($fileFull, $callerFull, [StringComparison]::OrdinalIgnoreCase)) { return }
    if ($env:SCRIPT_OWN_PANE -and $env:WEZTERM_PANE -and $weztermExe) {
        try { runNativeCommand $weztermExe @('cli', 'kill-pane', '--pane-id', $env:WEZTERM_PANE) 'wezterm' } catch { }
    }
    [Environment]::Exit(0)
}

function openUrl {
    param([Parameter(Mandatory)][string]$Url)
    Start-Process -FilePath $Url | Out-Null
}

function readVerFile {
    param([string]$LiteralPath = $version)
    $lines = @([IO.File]::ReadAllText($LiteralPath) -split "\r?\n")
    while ($lines.Count -lt 3) { $lines += '' }
    $lines
}

function writeFileNoBom {
    param([Parameter(Mandatory)][string]$LiteralPath, [Parameter(Mandatory)][string]$Content)
    [IO.File]::WriteAllText($LiteralPath, $Content, $noBom)
}

function writeVerFile {
    param(
        [Parameter(Mandatory)][string]$SemVer,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Tag,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Build
    )
    writeFileNoBom -LiteralPath $version -Content (($SemVer.Trim(), $Tag.Trim(), $Build.Trim()) -join "`n")
    if ($Tag.Trim()) {
        writeFileNoBom -LiteralPath $versionTag -Content ($Tag.Trim() + "`n")
    } elseif (Test-Path -LiteralPath $versionTag) {
        Remove-Item -LiteralPath $versionTag -Force
    }
}

function setVerBuild {
    param([Parameter(Mandatory)][string]$Platform)
    writeFileNoBom -LiteralPath $versionBuild -Content ($Platform.Trim() + "`n")
}

function checkVerBuild {
    param([string]$Architecture)
    if (Test-Path -LiteralPath $versionBuild) { return }
    setVerBuild ($Architecture ?? 'x64')
}

function getArchitecture {
    param([string[]]$FlagArgs)
    $found = @()
    foreach ($arg in @($FlagArgs)) {
        if ([string]::IsNullOrWhiteSpace("$arg")) { continue }
        switch -Regex ("$arg".Trim()) {
            '(?i)^(x64|--x64|-x64|--64|-64)$' { $found += 'x64' }
            '(?i)^(arm64|--arm64|-arm64|--arm|-arm)$' { $found += 'arm64' }
            '(?i)^(--help|-h)$' { return 'help' }
            '(?i)^(x86|--x86|-x86|--86|-86)$' { throw 'x86 is not a rustitles build target.' }
            default { throw "Unknown architecture flag: $arg" }
        }
    }
    $unique = @($found | Select-Object -Unique)
    if ($unique.Count -gt 1) { throw "Conflicting architecture flags: $($unique -join ', ')" }
    if ($unique.Count -eq 1) { return $unique[0] }
    $null
}

function getWindowsCargoTarget {
    param([string]$Architecture)
    if ($Architecture -eq 'arm64') { return 'aarch64-pc-windows-msvc' }
    'x86_64-pc-windows-msvc'
}

function runNativeCommand {
    param([Parameter(Mandatory)][string]$FilePath, [object[]]$ArgumentList, [string]$Name = $FilePath)
    & $FilePath @ArgumentList
    if ($LASTEXITCODE) { throw "$Name failed (exit $LASTEXITCODE)." }
}

function invokeNativeCommand {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(ValueFromRemainingArguments)][string[]]$Arguments
    )
    runNativeCommand -FilePath $Command -ArgumentList @($Arguments) -Name $Command
}

function writeRepoUtf8NoBomFile {
    param([Parameter(Mandatory)][string]$LiteralPath, [Parameter(Mandatory)][string]$Content)
    writeFileNoBom -LiteralPath $LiteralPath -Content $Content
}

function writeVersionFile {
    param([Parameter(Mandatory)][string]$SemVer, [string]$LiteralPath = $versionTxt)
    writeFileNoBom -LiteralPath $LiteralPath -Content "$SemVer`n"
}

function readVersionFile {
    param([string]$LiteralPath = $versionTxt)
    ([IO.File]::ReadAllText($LiteralPath)).Trim()
}

function syncVersionToCargoToml {
    param([Parameter(Mandatory)][string]$SemVer, [string]$LiteralPath = $crateManifest)
    $lines = [IO.File]::ReadAllLines($LiteralPath)
    $inPackage = $false
    for ($i = 0; $i -lt $lines.Length; $i++) {
        if ($lines[$i] -match '^\s*\[package\]\s*$') { $inPackage = $true }
        elseif ($lines[$i] -match '^\s*\[.*\]\s*$') { $inPackage = $false }
        elseif ($inPackage -and $lines[$i] -match '^\s*version\s*=\s*".*"\s*$') {
            $lines[$i] = "version = `"$SemVer`""
            break
        }
    }
    [IO.File]::WriteAllLines($LiteralPath, $lines, $noBom)
}

function syncVersionToConfigRs {
    param([Parameter(Mandatory)][string]$SemVer, [string]$LiteralPath = $configFile)
    $content = [IO.File]::ReadAllText($LiteralPath)
    $content = $content -replace 'pub const APP_VERSION: &str = "\d+\.\d+\.\d+";', "pub const APP_VERSION: &str = `"$SemVer`";"
    writeFileNoBom -LiteralPath $LiteralPath -Content $content
}

function getCargoPackageVersion {
    $content = [IO.File]::ReadAllText($crateManifest)
    $package = [regex]::Match($content, '(?ms)^\[package\].*?(?=^\[|\z)')
    if (-not $package.Success) { throw "[package] section not found: $crateManifest" }
    $match = [regex]::Match($package.Value, '(?m)^version\s*=\s*"([^"]+)"\s*$')
    if (-not $match.Success) { throw "Package version not found: $crateManifest" }
    $match.Groups[1].Value
}

function getConfigVersion {
    $content = [IO.File]::ReadAllText($configFile)
    $match = [regex]::Match($content, 'pub const APP_VERSION:\s*&str\s*=\s*"([^"]+)";')
    if (-not $match.Success) { throw "APP_VERSION not found: $configFile" }
    $match.Groups[1].Value
}

function assertVersionConsistency {
    $sot = (readVerFile)[0].Trim()
    $versions = @{
        '.version/version' = $sot
        'crate/src/version.txt' = (readVersionFile)
        'crate/Cargo.toml' = getCargoPackageVersion
        'crate/src/config.rs' = getConfigVersion
    }
    $mismatch = $versions.GetEnumerator() | Where-Object { $_.Value -ne $sot }
    if ($mismatch) {
        $details = $versions.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }
        throw "Version mismatch: $($details -join ', ')"
    }
}

function syncVersionConsumers {
    param([Parameter(Mandatory)][string]$SemVer)
    writeVersionFile -SemVer $SemVer
    syncVersionToCargoToml -SemVer $SemVer
    syncVersionToConfigRs -SemVer $SemVer
    assertVersionConsistency
}

function readBuildNotes {
    if (-not (Test-Path -LiteralPath $buildNotes)) { return @{ Title = ''; Body = '' } }
    $lines = @([IO.File]::ReadAllLines($buildNotes))
    if ($lines.Count -eq 0) { return @{ Title = ''; Body = '' } }
    $title = $lines[0].Trim()
    $body = ''
    if ($title -and $lines.Count -gt 1) {
        if (-not [string]::IsNullOrWhiteSpace($lines[1])) {
            throw 'buildNotes.txt must have one blank line after the first line, then the description.'
        }
        if ($lines.Count -gt 2) { $body = ($lines | Select-Object -Skip 2) -join "`n" }
    }
    @{ Title = $title; Body = $body.Trim() }
}

function rustitlesWinExeName {
    param([string]$Architecture = 'x64')
    if ($Architecture -eq 'arm64') { return 'rustitles_win_ARM64_portable.exe' }
    'rustitles_win_x64_portable.exe'
}

function rustitlesWinInstallerName {
    param([string]$Architecture = 'x64')
    if ($Architecture -eq 'arm64') { return 'rustitles_win_ARM64_installer.exe' }
    'rustitles_win_x64_installer.exe'
}

function getWindowsInstallerTargets {
    param([string]$Architecture)
    if (-not $Architecture) { return $windowsInstallerTargets }
    $target = @($windowsInstallerTargets | Where-Object Architecture -eq $Architecture)
    if (-not $target) { throw "No Windows installer target for architecture: $Architecture" }
    return ,$target[0]
}

function deleteDir {
    param([Parameter(Mandatory)][string]$Path)
    if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path -Recurse -Force }
}

function removeRootReleaseArtifacts {
    foreach ($pattern in @(
        'rustitles v*.exe'
        'rustitles installer.exe'
        'rustitles ARM64 installer.exe'
        'rustitles win *.exe'
        'rustitles_win_*.exe'
        'rustitles v*.AppImage'
        'rustitles v*.deb'
        'rustitles.AppImage'
        'rustitles.deb'
        'Rustitles v*.app'
        'rustitles.app'
        'Rustitles-v*-macOS.zip'
        'rustitles-macOS.zip'
        'rustitles_app-macOS.zip'
        'rustitles_app_macOS.zip'
    )) {
        Get-ChildItem -LiteralPath $repoRoot -Filter $pattern -Force -ErrorAction SilentlyContinue |
            ForEach-Object { Remove-Item -LiteralPath $_.FullName -Recurse -Force }
    }
}

function compileWindowsInstallers {
    param([string]$Architecture)
    $iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue)?.Source
    if (-not $iscc) { $iscc = 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' }
    if (-not (Test-Path -LiteralPath $iscc)) { throw "Inno Setup compiler not found: $iscc" }
    if (-not $Architecture) { deleteDir $installerOutput }
    New-Item -ItemType Directory -Path $installerOutput -Force | Out-Null
    foreach ($target in (getWindowsInstallerTargets $Architecture)) {
        if (-not (Test-Path -LiteralPath $target.ExePath -PathType Leaf)) {
            throw "Missing installer input: $($target.ExePath)"
        }
        runNativeCommand $iscc @("/DAppVersion=$versionContents", $target.InstallerScript) "ISCC $($target.Architecture)"
        $compiled = Join-Path $installerOutput $target.InstallerName
        if (-not (Test-Path -LiteralPath $compiled -PathType Leaf)) {
            throw "ISCC did not produce: $compiled"
        }
        Copy-Item -LiteralPath $compiled -Destination (Join-Path $publishFolder (rustitlesWinInstallerName $target.Architecture)) -Force
    }
}

function rustitlesLinuxAppImageName { 'rustitles.AppImage' }
function rustitlesLinuxDebName { 'rustitles.deb' }
function rustitlesMacAppName { 'rustitles.app' }
function rustitlesMacZipName { 'rustitles_app_macOS.zip' }

function encodeAssetUrl {
    param([Parameter(Mandatory)][string]$Name)
    "$appURL/releases/download/$tag/$([uri]::EscapeDataString($Name))"
}

function assertPublishArtifacts {
    param([string]$Architecture)
    $expected = @()
    if ($IsWindows) {
        foreach ($target in (getWindowsInstallerTargets $Architecture)) {
            $expected += @{ Name = rustitlesWinExeName $target.Architecture; Type = 'Leaf' }
            $expected += @{ Name = rustitlesWinInstallerName $target.Architecture; Type = 'Leaf' }
        }
    } elseif ($IsLinux) {
        $expected = @(
            @{ Name = rustitlesLinuxAppImageName; Type = 'Leaf' }
            @{ Name = rustitlesLinuxDebName; Type = 'Leaf' }
        )
    } elseif ($IsMacOS) {
        $expected += @{ Name = rustitlesMacAppName; Type = 'Container' }
        $expected += @{ Name = rustitlesMacZipName; Type = 'Leaf' }
    }
    foreach ($asset in $expected) {
        $path = Join-Path $publishFolder $asset.Name
        $pathType = if ($asset.Type -eq 'Container') { 'Container' } else { 'Leaf' }
        if (-not (Test-Path -LiteralPath $path -PathType $pathType)) {
            throw "Missing staged release asset: $path"
        }
    }
}

function assertReadmeReleaseLinks {
    if (-not (Test-Path -LiteralPath $readme -PathType Leaf)) { throw "README not found: $readme" }
    $content = [IO.File]::ReadAllText($readme)
    $start = '<!-- Quick Reference -->'
    $end = '<!-- End Quick Reference -->'
    $startIndex = $content.IndexOf($start, [StringComparison]::Ordinal)
    $endIndex = $content.IndexOf($end, [StringComparison]::Ordinal)
    if ($startIndex -lt 0 -or $endIndex -lt $startIndex) {
        throw 'README Quick Reference markers are missing.'
    }

    $expectedNames = @(
        (rustitlesWinExeName 'x64')
        (rustitlesWinExeName 'arm64')
        (rustitlesWinInstallerName 'x64')
        (rustitlesWinInstallerName 'arm64')
        (rustitlesMacZipName)
        (rustitlesLinuxAppImageName)
        (rustitlesLinuxDebName)
    )
    foreach ($name in $expectedNames) {
        if ($content.IndexOf((encodeAssetUrl $name), [StringComparison]::Ordinal) -lt 0) {
            throw "README is missing release link for: $name"
        }
    }
}

function invokeHostPackagers {
    param([string]$Architecture)
    if ($IsWindows) {
        $arches = if ($Architecture) { @($Architecture) } else { @('x64', 'arm64') }
        foreach ($arch in $arches) {
            runNativeCommand -FilePath (Join-Path $PSScriptRoot 'buildWin.ps1') -ArgumentList @($arch) -Name "buildWin.ps1 ($arch)"
        }
        compileWindowsInstallers $Architecture
        return
    }
    if ($IsLinux) {
        runNativeCommand -FilePath 'bash' -ArgumentList @(Join-Path $PSScriptRoot 'buildDebian.sh') -Name 'buildDebian.sh'
        return
    }
    if ($IsMacOS) {
        runNativeCommand -FilePath 'bash' -ArgumentList @(Join-Path $PSScriptRoot 'buildMac.sh') -Name 'buildMac.sh'
        return
    }
    throw 'Unsupported host operating system.'
}

function buildAll {
    param([string]$Architecture)
    $previousPipeline = $env:BASE_BUILD_PIPELINE
    try {
        $env:BASE_BUILD_PIPELINE = '1'
        Set-Location -LiteralPath $repoRoot
        removeRootReleaseArtifacts
        deleteDir $publishFolder
        assertVersionConsistency
        $buildArgs = if ($Architecture) { @($Architecture) } else { @() }
        if (-not $IsWindows) {
            runNativeCommand -FilePath (Join-Path $PSScriptRoot 'build.ps1') -ArgumentList $buildArgs -Name 'build.ps1'
        }
        $installerArgs = if ($Architecture) { @($Architecture) } else { @() }
        runNativeCommand -FilePath (Join-Path $PSScriptRoot 'buildInstaller.ps1') -ArgumentList $installerArgs -Name 'buildInstaller.ps1'
        assertPublishArtifacts -Architecture $Architecture
        runNativeCommand -FilePath (Join-Path $PSScriptRoot 'updateReadme.ps1') -ArgumentList @() -Name 'updateReadme.ps1'
        assertReadmeReleaseLinks
    } finally { $env:BASE_BUILD_PIPELINE = $previousPipeline }
}

Set-Location -LiteralPath $repoRoot
