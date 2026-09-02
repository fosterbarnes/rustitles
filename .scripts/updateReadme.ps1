#requires -Version 7.0
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scriptHelper.ps1')
Set-Location -LiteralPath $repoRoot
if (-not (Test-Path -LiteralPath $readme -PathType Leaf)) { throw "README not found: $readme" }
$content = [IO.File]::ReadAllText($readme)
$start = '<!-- Quick Reference -->'
$end = '<!-- End Quick Reference -->'
$startIndex = $content.IndexOf($start, [StringComparison]::Ordinal)
$endIndex = $content.IndexOf($end, [StringComparison]::Ordinal)
if ($startIndex -lt 0 -or $endIndex -lt $startIndex) { throw 'README Quick Reference markers are missing.' }
$assetBase = 'https://raw.githubusercontent.com/fosterbarnes/res/main/btn'
$assetBaseRefs = 'https://raw.githubusercontent.com/fosterbarnes/res/refs/heads/main/btn'
$latestDeb = "https://github.com/fosterbarnes/rustitles/releases/latest/download/$(rustitlesLinuxDebName)"
function buttonCell([string]$AssetName, [string]$ImageUrl, [string]$Alt) {
    @(
        '      <td align="center" valign="top">'
        "        <a href=`"$(encodeAssetUrl $AssetName)`">"
        "          <img src=`"$ImageUrl`" width=`"180`" height=`"auto`" alt=`"$Alt`"/>"
        '        </a>'
        '      </td>'
    )
}
$lines = @(
    $start
    ''
    '### Windows'
    ''
    '<table border="0">'
    '  <tbody>'
    '    <tr>'
    buttonCell (rustitlesWinInstallerName 'x64') "$assetBase/x64Installer.svg" 'x64 installer'
    buttonCell (rustitlesWinExeName 'x64') "$assetBase/x64Portable.svg" 'x64 portable'
    '    </tr>'
    '    <tr>'
    buttonCell (rustitlesWinInstallerName 'arm64') "$assetBase/arm64.svg" 'ARM64 installer'
    buttonCell (rustitlesWinExeName 'arm64') "$assetBase/arm64Portable.svg" 'ARM64 portable'
    '    </tr>'
    '  </tbody>'
    '</table>'
    ''
    '### macOS'
    ''
    '<table border="0">'
    '  <tbody>'
    '    <tr>'
    buttonCell (rustitlesMacZipName) "$assetBase/appleArm.svg" 'Universal macOS app'
    buttonCell (rustitlesMacZipName) "$assetBase/appleIntel.svg" 'Universal macOS app'
    '    </tr>'
    '  </tbody>'
    '</table>'
    ''
    '### Linux'
    ''
    '<table border="0">'
    '  <tbody>'
    '    <tr>'
    buttonCell (rustitlesLinuxDebName) "$assetBaseRefs/deb.svg" 'Linux DEB package'
    buttonCell (rustitlesLinuxAppImageName) "$assetBaseRefs/appImage.svg" 'Linux AppImage'
    '    </tr>'
    '  </tbody>'
    '</table>'
    ''
    'Or install the `.deb` directly from the terminal:'
    ''
    '```bash'
    "wget $latestDeb"
    "sudo apt install ./$(rustitlesLinuxDebName)"
    '```'
    ''
    $end
)
$prefix = $content.Substring(0, $startIndex)
$suffix = $content.Substring($endIndex + $end.Length)
writeFileNoBom -LiteralPath $readme -Content ($prefix + ($lines -join "`n") + $suffix)
closeOut 3
