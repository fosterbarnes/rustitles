# Define variables
$rootFolder = "C:\Users\Foster\Documents\Rust\rustitles"
$releaseEXE = "$rootFolder\target\release\rustitles.exe"
$versionTXT = "$rootFolder\src\version.txt"
$version = Get-Content $versionTXT -Raw | ForEach-Object { $_.Trim() }

# Update version in Cargo.toml
$cargoTomlPath = "$rootFolder\Cargo.toml"
$cargoLines = Get-Content $cargoTomlPath
$inPackageSection = $false
for ($i = 0; $i -lt $cargoLines.Length; $i++) {
    if ($cargoLines[$i] -match '^\s*\[package\]\s*$') {
        $inPackageSection = $true
    }
    elseif ($cargoLines[$i] -match '^\s*\[.*\]\s*$' -and $cargoLines[$i] -notmatch '^\s*\[package\]\s*$') {
        $inPackageSection = $false
    }
    elseif ($inPackageSection -and $cargoLines[$i] -match '^\s*version\s*=\s*".*"\s*$') {
        $cargoLines[$i] = "version = `"$version`""
        break
    }
}
Set-Content $cargoTomlPath -Value $cargoLines

# Update version in src/config.rs
$configPath = "$rootFolder\src\config.rs"
$configContent = Get-Content $configPath -Raw
$configContent = $configContent -replace 'pub const APP_VERSION: &str = "\d+\.\d+\.\d+";', "pub const APP_VERSION: &str = `"$version`";"
Set-Content $configPath -Value $configContent -NoNewline

Write-Host "Version updated in Cargo.toml and config.rs"

# Delete old .exe and log
Remove-Item "rustitles v*.exe"
Remove-Item "rustitles_lo*.txt"

# Compile
cargo build --release

# Copy new .exe to /rustitles
Copy-Item $releaseEXE $rootFolder

# Rename rustitles.exe to reflect version number
$finalExeName = "rustitles v$version.exe"
Rename-Item "$rootFolder\rustitles.exe" $finalExeName
Write-Host "Executable renamed to: $finalExeName"
Write-Host "Build completed successfully!"