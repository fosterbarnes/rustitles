#!/bin/bash
set -euo pipefail

ROOT_FOLDER="$(cd "$(dirname "$0")/.." && pwd)"
CRATE_DIR="$ROOT_FOLDER/crate"
VERSION_FILE="$ROOT_FOLDER/.version/version"
PUBLISH_DIR="$ROOT_FOLDER/publish"
cd "$ROOT_FOLDER"

VERSION=$(sed -n '1p' "$VERSION_FILE" | tr -d '\r' | xargs)
if [ -z "$VERSION" ]; then
    echo "Error: version source is empty: $VERSION_FILE" >&2
    exit 1
fi

CARGO_TOML="$CRATE_DIR/Cargo.toml"
VERSION_TXT="$CRATE_DIR/src/version.txt"
CRATE_VERSION=$(sed -n '1p' "$VERSION_TXT" | tr -d '\r' | xargs)
CARGO_VERSION=$(awk '
/^\[package\]/ { in_package = 1; next }
/^\[/ && !/^\[package\]/ { in_package = 0 }
in_package && /^version = / { gsub(/"/, "", $3); print $3; exit }
' "$CARGO_TOML" | tr -d '\r')
CONFIG_RS="$CRATE_DIR/src/config.rs"
CONFIG_VERSION=$(sed -n 's/^pub const APP_VERSION: &str = "\([^"]*\)";/\1/p' "$CONFIG_RS" | tr -d '\r')
if [ "$CRATE_VERSION" != "$VERSION" ] || [ "$CARGO_VERSION" != "$VERSION" ] || [ "$CONFIG_VERSION" != "$VERSION" ]; then
    echo "Error: version consumers do not match $VERSION" >&2
    exit 1
fi

echo "Building Rustitles v${VERSION} for macOS (aarch64 + x86_64)..."

if ! command -v cargo &>/dev/null; then
    echo "Rust not found. Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "Rust installed."
fi

export PATH="$HOME/.cargo/bin:$PATH"
INSTALLED=$(rustup target list --installed)
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    if ! echo "$INSTALLED" | grep -qx "$target"; then
        echo "Installing Rust target $target..."
        rustup target add "$target"
    fi
done

echo "Building for aarch64-apple-darwin (Apple Silicon)..."
cargo build --release --target aarch64-apple-darwin --manifest-path "$CRATE_DIR/Cargo.toml"
echo "Building for x86_64-apple-darwin (Intel)..."
cargo build --release --target x86_64-apple-darwin --manifest-path "$CRATE_DIR/Cargo.toml"

echo "Creating .app bundle..."
mkdir -p "$PUBLISH_DIR"
APP_NAME="rustitles.app"
APP_PATH="$PUBLISH_DIR/$APP_NAME"
ZIP_NAME="rustitles_app_macOS.zip"
ZIP_PATH="$PUBLISH_DIR/$ZIP_NAME"
CONTENTS_DIR="$APP_PATH/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
rm -rf "$PUBLISH_DIR"/Rustitles\ v*.app "$APP_PATH"
rm -f "$PUBLISH_DIR"/Rustitles-v*-macOS.zip "$PUBLISH_DIR"/rustitles-macOS.zip "$ZIP_PATH"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

lipo -create -output "$MACOS_DIR/rustitles" \
    "$ROOT_FOLDER/target/aarch64-apple-darwin/release/rustitles" \
    "$ROOT_FOLDER/target/x86_64-apple-darwin/release/rustitles"

ICON_TMP=$(mktemp -d)
ICONSET_DIR="$ICON_TMP/rustitles.iconset"
trap 'rm -rf "$ICON_TMP"' EXIT

PNG=""
[ -f "$CRATE_DIR/resources/rustitles_mac_icon.png" ] && PNG="$CRATE_DIR/resources/rustitles_mac_icon.png"
[ -z "$PNG" ] && [ -f "$CRATE_DIR/resources/rustitles_icon.png" ] && PNG="$CRATE_DIR/resources/rustitles_icon.png"

if [ -n "$PNG" ]; then
    mkdir -p "$ICONSET_DIR"
    sips -z 16 16 "$PNG" --out "$ICONSET_DIR/icon_16x16.png"
    sips -z 32 32 "$PNG" --out "$ICONSET_DIR/icon_16x16@2x.png"
    sips -z 32 32 "$PNG" --out "$ICONSET_DIR/icon_32x32.png"
    sips -z 64 64 "$PNG" --out "$ICONSET_DIR/icon_32x32@2x.png"
    sips -z 128 128 "$PNG" --out "$ICONSET_DIR/icon_128x128.png"
    sips -z 256 256 "$PNG" --out "$ICONSET_DIR/icon_128x128@2x.png"
    sips -z 256 256 "$PNG" --out "$ICONSET_DIR/icon_256x256.png"
    sips -z 512 512 "$PNG" --out "$ICONSET_DIR/icon_256x256@2x.png"
    sips -z 512 512 "$PNG" --out "$ICONSET_DIR/icon_512x512.png"
    sips -z 1024 1024 "$PNG" --out "$ICONSET_DIR/icon_512x512@2x.png"
    iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/rustitles.icns"
elif [ -f "$CRATE_DIR/resources/rustitles.icns" ]; then
    cp "$CRATE_DIR/resources/rustitles.icns" "$RESOURCES_DIR/rustitles.icns"
fi

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
    <string>rustitles</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
</dict>
</plist>
PLIST

chmod +x "$MACOS_DIR/rustitles"
echo "Creating macOS release archive..."
ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$ZIP_PATH"
echo "Build complete!"
echo "App bundle staged at: $APP_PATH"
echo "Release archive staged at: $ZIP_PATH"
