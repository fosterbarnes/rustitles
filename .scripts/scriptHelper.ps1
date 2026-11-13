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
# Private, gitignored word list for version tags (copied from oc); only the maintainer's clone has it.
$yapWords = Join-Path $repoRoot '.res' 'yapBot.json'
$exeSuffix = if ($IsWindows) { '.exe' } else { '' }
$installerFolder = Join-Path $repoRoot '.installer'
$installerOutput = Join-Path $installerFolder 'Output'
$publishFolder = Join-Path $repoRoot 'publish'
$publishBuildFolder = Join-Path $publishFolder 'build'
$appPublisher = 'fosterbarnes'
$appURL = "https://github.com/$appPublisher/$projectName"
$ghRepo = "$appPublisher/$projectName"
$versionContents = ([IO.File]::ReadAllText($version) -split '\r?\n')[0].Trim()
# GitHub release tag. The camelCase word in .version/versionTag names the version in commit subjects.
$tag = "v$versionContents"
$noBom = [Text.UTF8Encoding]::new($false)
$weztermExe = (Get-Command "wezterm$exeSuffix" -ErrorAction SilentlyContinue)?.Source
# Commit subject labels in decision order; AGENTS.md (buildNotes.txt section) defines each.
$commitLabels = 'fix', 'feat.', 'patch', 'perf.', 'deps.', 'scripts', 'cleanup', 'docs', 'misc.'
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

# Ends a script started with `pwsh -File`; a no-op for nested `& other.ps1` calls, pipeline runs, and
# an open prompt. SCRIPT_OWN_PANE is set by the oc sidebar for panes it made for one script.
function closeOut {
    param([int]$Seconds = 5)
    if ($env:BASE_BUILD_PIPELINE) { return }
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
    if ($Seconds -lt 0) { $Seconds = 0 }
    if ($Seconds -gt 0) {
        $pad = "closing after $Seconds seconds..."
        foreach ($n in $Seconds..1) {
            writeClearedLine -Text "closing after $n seconds..." -PadWidth $pad.Length -NoNewline
            Start-Sleep -Seconds 1
        }
        writeClearedLine -Text 'closing...' -PadWidth $pad.Length
    }
    # A failed or missing wezterm call must not stop the exit below.
    try {
        if ($env:SCRIPT_OWN_PANE -and $env:WEZTERM_PANE -and $weztermExe) {
            & $weztermExe @('cli', 'kill-pane', '--pane-id', $env:WEZTERM_PANE)
        }
    } catch { }
    [Environment]::Exit(0)
}

function openUrl {
    param([Parameter(Mandatory)][string]$Url)
    Start-Process -FilePath $Url | Out-Null
}

function writeFileNoBom {
    param([Parameter(Mandatory)][string]$LiteralPath, [Parameter(Mandatory)][string]$Content)
    [IO.File]::WriteAllText($LiteralPath, $Content, $noBom)
}

function requireVersion {
    $contents = [IO.File]::ReadAllText($version).Trim()
    if ($contents -notmatch '^\d+\.\d+\.\d+$') { throw "Invalid $version`: expected one version in X.Y.Z format" }
    $contents
}

# Writes .version/version and every file that repeats it (Cargo.toml, version.txt, config.rs).
function setVersion {
    param([Parameter(Mandatory)][string]$SemVer)
    writeFileNoBom -LiteralPath $version -Content "$SemVer`n"
    syncVersionConsumers $SemVer
}

# The version one part higher than .version/version; lower parts reset to 0.
function getNextVersion {
    param([Parameter(Mandatory)][ValidateSet('major', 'minor', 'patch')][string]$Part)
    $major, $minor, $patch = @((requireVersion).Split('.') | ForEach-Object { [int]$_ })
    switch ($Part) {
        'major' { "$($major + 1).0.0" }
        'minor' { "$major.$($minor + 1).0" }
        'patch' { "$major.$minor.$($patch + 1)" }
    }
}

# Bumps one version part, syncs every version consumer, and rolls a new version tag.
function bumpVersionPart {
    param([Parameter(Mandatory)][ValidateSet('major', 'minor', 'patch')][string]$Part)
    $current = requireVersion
    $next = getNextVersion $Part
    setVersion $next
    $newTag = newVersionTag
    Write-Host "Version $current -> $next ($newTag)"
    $next
}

function getHeadVersion {
    $headVersion = (& git -C $repoRoot show HEAD:.version/version | Out-String).Trim()
    if ($LASTEXITCODE) { throw "git show HEAD version failed (exit $LASTEXITCODE)." }
    $headVersion
}

# A push with code changes needs a version bump unless the user declines one (agentPush -NoBump);
# docs and notes alone do not. Returns the changed or untracked code paths when the version still
# matches HEAD, else nothing.
function getUnbumpedCodeChanges {
    if ((getHeadVersion) -ne (requireVersion)) { return @() }
    $changed = @(& git -C $repoRoot diff --name-only HEAD)
    if ($LASTEXITCODE) { throw "git diff failed (exit $LASTEXITCODE)." }
    $changed += @(& git -C $repoRoot ls-files -o --exclude-standard)
    if ($LASTEXITCODE) { throw "git ls-files failed (exit $LASTEXITCODE)." }
    @($changed | Where-Object { $_ -notmatch '(^docs/|\.md$|^buildNotes\.txt$|^\.version/)' })
}

function newVersionTag { setVersionTag (getVersionTags 1) }

# Returns $Count distinct random camelCase two-word tags without writing anything.
function getVersionTags {
    param([int]$Count = 1)
    if (-not (Test-Path -LiteralPath $yapWords)) {
        throw "Missing word list $yapWords. It is private and gitignored; copy it from oc's .res folder."
    }
    $words = @(Get-Content -LiteralPath $yapWords -Raw | ConvertFrom-Json | Where-Object { $_ -match '^[A-Za-z]+$' })
    if (-not $words.Count) { throw "No usable words found in $yapWords." }
    $tags = [Collections.Generic.HashSet[string]]::new()
    while ($tags.Count -lt $Count) {
        $first = $words[[Security.Cryptography.RandomNumberGenerator]::GetInt32($words.Count)]
        $second = $words[[Security.Cryptography.RandomNumberGenerator]::GetInt32($words.Count)]
        [void]$tags.Add($first.ToLowerInvariant() + $second.Substring(0, 1).ToUpperInvariant() + $second.Substring(1).ToLowerInvariant())
    }
    @($tags)
}

function setVersionTag {
    param([Parameter(Mandatory)][string]$Tag)
    if ($Tag -cnotmatch '^[a-z]+[A-Z][a-z]*$') { throw "Invalid tag '$Tag': expected one camelCase two-word tag" }
    writeFileNoBom -LiteralPath $versionTag -Content "$Tag`n"
    $Tag
}

function requireVersionTag {
    if (-not (Test-Path -LiteralPath $versionTag)) {
        throw "Missing $versionTag`: run .scripts/bumpVersion.ps1 -RerollTag to generate one."
    }
    $contents = [IO.File]::ReadAllText($versionTag).Trim()
    if ($contents -cnotmatch '^[a-z]+[A-Z][a-z]*$') { throw "Invalid $versionTag`: expected one camelCase two-word tag" }
    $contents
}

# True when buildNotes.txt differs from HEAD or is not tracked yet. git diff --quiet exits 1 for a
# difference; any other nonzero exit is a git failure, not a changed file.
function testBuildNotesChanged {
    $tracked = @(& git -C $repoRoot ls-files -- $buildNotes)
    if ($LASTEXITCODE) { throw "git ls-files failed (exit $LASTEXITCODE)." }
    if (-not $tracked) { return $true }
    & git -C $repoRoot diff --quiet HEAD -- $buildNotes
    switch ($LASTEXITCODE) {
        0 { return $false }
        1 { return $true }
        default { throw "git diff failed (exit $LASTEXITCODE)." }
    }
}

# Throws unless $Subject is "<Prefix> [<label> > <summary>]", optionally "; <label> > <summary>".
# Summaries: no edge spaces, no trailing period, none of [ ] > ;. .NET regex is case-sensitive.
function assertCommitSubject {
    param([Parameter(Mandatory)][string]$Subject, [Parameter(Mandatory)][string]$Prefix)
    $labels = ($commitLabels | ForEach-Object { [regex]::Escape($_) }) -join '|'
    $segment = "($labels) > [^\s\[\]>;](?:[^\[\]>;]*[^\s.\[\]>;])?"
    $match = [regex]::Match($Subject, "^$([regex]::Escape($Prefix)) \[$segment(?:; $segment)?\]$")
    if (-not $match.Success) {
        throw "Line 1 must be '$Prefix [<label> > <summary>]', optionally '; <label> > <summary>'. " +
            "Labels: $($commitLabels -join ' ')"
    }
    $first, $second = $match.Groups[1].Value, $match.Groups[2].Value
    if ($second -and $first -eq $second) { throw "Line 1 repeats label '$first'; merge the segments." }
    if ($second -and 'misc.' -in $first, $second) { throw "'misc.' must be the only segment." }
    if ($Subject -match '[^\x20-\x7E]') { throw 'Line 1 must be plain ASCII.' }
    if ($Subject.Length -gt 72) { throw "Line 1 is $($Subject.Length) characters; the limit is 72." }
}

# Makes modified and untracked files match their .gitattributes eol (*.ps1 CRLF, *.sh LF). Files
# without an eol attribute are left alone (Rust sources stay LF). Binary and non-UTF-8 files are skipped.
function setLineEndings {
    $paths = @(git -C $repoRoot ls-files -m -o --exclude-standard)
    if ($LASTEXITCODE) { throw "git ls-files failed (exit $LASTEXITCODE)." }
    if (-not $paths) { return }
    $attrs = @($paths | git -C $repoRoot check-attr --stdin eol)
    if ($LASTEXITCODE) { throw "git check-attr failed (exit $LASTEXITCODE)." }
    $strictUtf8 = [Text.UTF8Encoding]::new($false, $true)
    $count = 0
    foreach ($line in $attrs) {
        if ($line -notmatch '^(.*): eol: (lf|crlf)$') { continue }
        $path, $eol = $Matches[1], $Matches[2]
        $full = Join-Path $repoRoot $path
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
        $bytes = [IO.File]::ReadAllBytes($full)
        if ([Array]::IndexOf($bytes, [byte]0) -ge 0) { continue }
        $bom = [byte[]]@()
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            $bom = [byte[]](0xEF, 0xBB, 0xBF)
        }
        try { $text = $strictUtf8.GetString($bytes, $bom.Length, $bytes.Length - $bom.Length) } catch { continue }
        $fixed = $eol -eq 'lf' ? $text.Replace("`r`n", "`n") : [regex]::Replace($text, '(?<!\r)\n', "`r`n")
        if ($fixed -ceq $text) { continue }
        [IO.File]::WriteAllBytes($full, [byte[]]($bom + $noBom.GetBytes($fixed)))
        $count++
    }
    if ($count) { Write-Host "Fixed line endings in $count file(s)." }
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
    # Clear a stale code first: a .ps1 run in-process leaves $LASTEXITCODE untouched on success.
    $global:LASTEXITCODE = 0
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
    $sot = requireVersion
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
        runNativeCommand -FilePath 'bash' -ArgumentList @(Join-Path $PSScriptRoot 'buildLinux.sh') -Name 'buildLinux.sh'
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
        # The host packagers (buildWin.ps1, buildMac.sh, buildLinux.sh) run their own release builds.
        $installerArgs = if ($Architecture) { @($Architecture) } else { @() }
        runNativeCommand -FilePath (Join-Path $PSScriptRoot 'buildInstaller.ps1') -ArgumentList $installerArgs -Name 'buildInstaller.ps1'
        assertPublishArtifacts -Architecture $Architecture
        runNativeCommand -FilePath (Join-Path $PSScriptRoot 'updateReadme.ps1') -ArgumentList @() -Name 'updateReadme.ps1'
        assertReadmeReleaseLinks
    } finally { $env:BASE_BUILD_PIPELINE = $previousPipeline }
}

Set-Location -LiteralPath $repoRoot
