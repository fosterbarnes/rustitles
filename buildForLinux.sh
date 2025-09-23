#!/usr/bin/env bash
set -e  # Exit on any error

# Define variables
ROOT_FOLDER="/home/foster/Desktop/rustitles"
ORIGINAL_PROJECT_FOLDER="/mnt/hgfs/Rust/rustitles"
VERSION_TXT="$ROOT_FOLDER/src/version.txt"

# Read version from version.txt
VERSION=$(cat "$VERSION_TXT" | tr -d '\n' | tr -d '\r' | xargs)
echo "Updating version to: '$VERSION'"

# Update version in Cargo.toml (only the package version, not dependency versions)
CARGO_TOML="$ROOT_FOLDER/Cargo.toml"
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
CONFIG_RS="$ROOT_FOLDER/src/config.rs"
echo "Updating version in config.rs"
sed -i 's/^pub const APP_VERSION: &str = ".*";/pub const APP_VERSION: \&str = "'"$VERSION"'";/' "$CONFIG_RS"

echo "Version updated in Cargo.toml and config.rs"

# 1. Clear the contents of AppDir/usr/bin
echo "1. Clearing contents of /AppDir/usr/bin"
BIN_DIR="$ROOT_FOLDER/AppDir/usr/bin"
rm -rf "${BIN_DIR:?}/"*  # The :? prevents accidental deletion if $BIN_DIR is empty

# 2. Copy rustitles_icon.png to AppDir, replacing if needed
echo "2. Copying icon"
cp -f "$ROOT_FOLDER/resources/rustitles_icon.png" "$ROOT_FOLDER/AppDir/rustitles_icon.png"

# 3. Copy rustitles.desktop to AppDir
echo "3. Copying rustitles.desktop"
cp -f "$ROOT_FOLDER/AppImageTool/rustitles.desktop" "$ROOT_FOLDER/AppDir/rustitles.desktop"

# 4. cd to root folder
echo "4. cd to root folder"
cd "$ROOT_FOLDER"

# 5. Delete old release
echo "5. Deleting old releases"
rm -f "$ROOT_FOLDER/target/release/rustitles_linux" "$ROOT_FOLDER/target/release/rustitles" "$ROOT_FOLDER/rustitles*.AppImage"

# 6. Run cargo build --release
echo "6. Building..."
cargo build --release

# 7. Rename binary
echo "7. Renaming to rustitles_linux"
mv -f target/release/rustitles target/release/rustitles_linux

# 8. Copy the built binary to AppDir/usr/bin
echo "8. Copy rustitles_linux to AppDir"
cp -f "target/release/rustitles_linux" "AppDir/usr/bin/"

# 9. Run linuxdeploy
echo "9. Run linuxdeploy"
./linuxdeploy-x86_64.AppImage --appdir AppDir --desktop-file AppDir/rustitles.desktop --icon-file AppDir/rustitles_icon.png

#10. Run appimagetool
echo "10. Run appimagetool"
./appimagetool-x86_64.AppImage AppDir

#11. Delete old versioned AppImages and rename new one
echo "11. Deleting old versioned AppImages"
rm -f "$ROOT_FOLDER/rustitles v*.AppImage"
echo "11b. Rename AppImage to include version"
mv -f "$ROOT_FOLDER/Rustitles-x86_64.AppImage" "$ROOT_FOLDER/rustitles v$VERSION.AppImage"

#12. Remove old AppImages from original project folder
echo "12. Deleting old AppImages from original project folder"
cd "$ORIGINAL_PROJECT_FOLDER"
rm -f rustitles\ v*.AppImage

#13. Copy new AppImage to original project folder
echo "13. Copying new AppImage to original project folder"
cp -f "$ROOT_FOLDER/rustitles v$VERSION.AppImage" "$ORIGINAL_PROJECT_FOLDER/"

echo "Done!!!"