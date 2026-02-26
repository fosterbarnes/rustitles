#!/usr/bin/env bash
set -e  # Exit on any error

# Define variables (run from repo root; crate contains Cargo package)
ROOT_FOLDER="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$ROOT_FOLDER/crate"
ORIGINAL_PROJECT_FOLDER="${ORIGINAL_PROJECT_FOLDER:-$ROOT_FOLDER}"
VERSION_TXT="$CRATE_DIR/src/version.txt"

# Read version from version.txt
VERSION=$(cat "$VERSION_TXT" | tr -d '\n' | tr -d '\r' | xargs)
echo "Updating version to: '$VERSION'"

# Update version in Cargo.toml (only the package version, not dependency versions)
CARGO_TOML="$CRATE_DIR/Cargo.toml"
echo "Updating version in Cargo.toml"
awk '
/^\[package\]/ { in_package = 1; print; next }
/^\[/ && !/^\[package\]/ { in_package = 0 }
in_package && /^version = / { 
    gsub(/"[^"]*"/, "\"'"$VERSION"'\"")
}
{ print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# Update version in src/config.rs
CONFIG_RS="$CRATE_DIR/src/config.rs"
echo "Updating version in config.rs"
sed -i 's/^pub const APP_VERSION: &str = ".*";/pub const APP_VERSION: \&str = "'"$VERSION"'";/' "$CONFIG_RS"

echo "Version updated in Cargo.toml and config.rs"

# 1. Create AppDir structure and clear bin contents
echo "1. Setting up AppDir"
BIN_DIR="$ROOT_FOLDER/AppDir/usr/bin"
mkdir -p "$BIN_DIR"
rm -rf "${BIN_DIR:?}/"*

# 2. Copy rustitles_icon.png to AppDir, replacing if needed
echo "2. Copying icon"
cp -f "$CRATE_DIR/resources/rustitles_icon.png" "$ROOT_FOLDER/AppDir/rustitles_icon.png"

# 3. Create and copy rustitles.desktop to AppDir
echo "3. Setting up .desktop file"
mkdir -p "$ROOT_FOLDER/AppImageTool"
cat > "$ROOT_FOLDER/AppImageTool/rustitles.desktop" << 'DESKTOP'
[Desktop Entry]
Name=Rustitles
Exec=rustitles_linux
Icon=rustitles_icon
Type=Application
Categories=Utility;
DESKTOP
cp -f "$ROOT_FOLDER/AppImageTool/rustitles.desktop" "$ROOT_FOLDER/AppDir/rustitles.desktop"

# 4. cd to root folder
echo "4. cd to root folder"
cd "$ROOT_FOLDER"

# 5. Delete old releases
echo "5. Deleting old releases"
rm -f "$CRATE_DIR/target/release/rustitles_linux" "$CRATE_DIR/target/release/rustitles"
rm -f "$ROOT_FOLDER/"rustitles*.AppImage "$ROOT_FOLDER/"rustitles\ v*.AppImage
rm -f "$ROOT_FOLDER/"rustitles*.deb "$ROOT_FOLDER/"rustitles\ v*.deb

# 6. Run cargo build --release
echo "6. Building..."
cargo build --release --manifest-path "$CRATE_DIR/Cargo.toml"

# 7. Rename binary
echo "7. Renaming to rustitles_linux"
mv -f "$CRATE_DIR/target/release/rustitles" "$CRATE_DIR/target/release/rustitles_linux"

# 8. Copy the built binary to AppDir/usr/bin
echo "8. Copy rustitles_linux to AppDir"
cp -f "$CRATE_DIR/target/release/rustitles_linux" "AppDir/usr/bin/"

# 9. Ensure linuxdeploy is available
echo "9. Checking for linuxdeploy"
if [ ! -f "./linuxdeploy-x86_64.AppImage" ]; then
    echo "   linuxdeploy not found, downloading..."
    wget -q https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
    chmod +x linuxdeploy-x86_64.AppImage
fi

# 10. Ensure appimagetool is available
echo "10. Checking for appimagetool"
if [ ! -f "./appimagetool-x86_64.AppImage" ]; then
    echo "   appimagetool not found, downloading..."
    wget -q https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x appimagetool-x86_64.AppImage
fi

# 11. Run linuxdeploy
echo "11. Run linuxdeploy"
./linuxdeploy-x86_64.AppImage --appdir AppDir --desktop-file AppDir/rustitles.desktop --icon-file AppDir/rustitles_icon.png

# 12. Run appimagetool
echo "12. Run appimagetool"
./appimagetool-x86_64.AppImage AppDir

# 13. Rename AppImage to include version
echo "13. Rename AppImage to include version"
mv -f "$ROOT_FOLDER/Rustitles-x86_64.AppImage" "$ROOT_FOLDER/rustitles v$VERSION.AppImage"

# 14. Build .deb package
echo "14. Building .deb package"
DEB_DIR="$ROOT_FOLDER/deb-pkg"
rm -rf "$DEB_DIR"
mkdir -p "$DEB_DIR/DEBIAN"
mkdir -p "$DEB_DIR/usr/bin"
mkdir -p "$DEB_DIR/usr/share/applications"
mkdir -p "$DEB_DIR/usr/share/icons/hicolor/256x256/apps"

cp -f "$CRATE_DIR/target/release/rustitles_linux" "$DEB_DIR/usr/bin/rustitles"
cp -f "$CRATE_DIR/resources/rustitles_icon.png" "$DEB_DIR/usr/share/icons/hicolor/256x256/apps/rustitles.png"

cat > "$DEB_DIR/usr/share/applications/rustitles.desktop" << 'DESKTOP'
[Desktop Entry]
Name=Rustitles
Comment=Subtitle Downloader Tool
Exec=rustitles
Icon=rustitles
Terminal=false
Type=Application
Categories=Utility;Video;
DESKTOP

cat > "$DEB_DIR/DEBIAN/control" << EOF
Package: rustitles
Version: $VERSION
Section: utils
Priority: optional
Architecture: amd64
Depends: python3, python3-pip
Maintainer: Foster Barnes
Description: Rustitles - Subtitle Downloader Tool
 A desktop application for automatically downloading subtitles for video files.
EOF

dpkg-deb --build "$DEB_DIR" "$ROOT_FOLDER/rustitles v${VERSION}.deb"
rm -rf "$DEB_DIR"
echo "14b. .deb package created: rustitles v${VERSION}.deb"

# 15. Remove old builds from original project folder
echo "15. Deleting old builds from original project folder"
cd "$ORIGINAL_PROJECT_FOLDER"
rm -f rustitles\ v*.AppImage
rm -f rustitles\ v*.deb

# 16. Copy new builds to original project folder
echo "16. Copying new builds to original project folder"
cp -f "$ROOT_FOLDER/rustitles v$VERSION.AppImage" "$ORIGINAL_PROJECT_FOLDER/"
cp -f "$ROOT_FOLDER/rustitles v$VERSION.deb" "$ORIGINAL_PROJECT_FOLDER/"

echo "Done!!!"