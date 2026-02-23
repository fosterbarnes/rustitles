#!/bin/bash
# Build script for macOS
# This script builds Rustitles and creates a .app bundle

set -e

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "Building Rustitles v${VERSION} for macOS..."

# Build the release binary
cargo build --release

echo "Creating .app bundle..."

APP_NAME="Rustitles.app"
CONTENTS_DIR="$APP_NAME/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

rm -rf "$APP_NAME"

mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"

cp target/release/rustitles "$MACOS_DIR/"

cat > "$CONTENTS_DIR/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Rustitles</string>
    <key>CFBundleDisplayName</key>
    <string>Rustitles</string>
    <key>CFBundleIdentifier</key>
    <string>com.fosterbarnes.rustitles</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleExecutable</key>
    <string>rustitles</string>
    <key>CFBundleIconFile</key>
    <string>rustitles_icon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
</dict>
</plist>
PLIST

if [ -f "resources/rustitles_icon.png" ]; then
    cp resources/rustitles_icon.png "$RESOURCES_DIR/"
fi

chmod +x "$MACOS_DIR/rustitles"

echo "Build complete!"
echo "App bundle created at: $APP_NAME"
echo ""
echo "To run: open $APP_NAME"
echo "To create DMG: hdiutil create -volname Rustitles -srcfolder $APP_NAME -ov -format UDZO Rustitles-v${VERSION}-macOS.dmg"
