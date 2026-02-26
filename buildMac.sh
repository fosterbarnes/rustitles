#!/bin/bash
# Build script for macOS (Apple Silicon + Intel universal binary)
# Installs Rust (rustup) and macOS targets if missing.

set -e

ROOT_FOLDER="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$ROOT_FOLDER/crate"
VERSION_TXT="$CRATE_DIR/src/version.txt"

VERSION=$(cat "$VERSION_TXT" | tr -d '\n' | tr -d '\r' | xargs)
echo "Building Rustitles v${VERSION} for macOS (aarch64 + x86_64)..."

# Install Rust if not present
if ! command -v cargo &>/dev/null; then
  echo "Rust not found. Installing rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  echo "Rust installed."
fi

# Ensure cargo is on PATH (e.g. if Rust was just installed in this script)
export PATH="$HOME/.cargo/bin:$PATH"

# Install macOS targets if missing (required for universal binary)
INSTALLED=$(rustup target list --installed)
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  if ! echo "$INSTALLED" | grep -qx "$target"; then
    echo "Installing Rust target $target..."
    rustup target add "$target"
  fi
done

# Update version in Cargo.toml (only the package version, not dependency versions)
CARGO_TOML="$CRATE_DIR/Cargo.toml"
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
sed -i '' 's/^pub const APP_VERSION: &str = ".*";/pub const APP_VERSION: \&str = "'"$VERSION"'";/' "$CONFIG_RS"

echo "Version updated in Cargo.toml and config.rs"

# Build for both architectures (universal binary for Intel + Apple Silicon)
echo "Building for aarch64-apple-darwin (Apple Silicon)..."
cargo build --release --target aarch64-apple-darwin --manifest-path "$CRATE_DIR/Cargo.toml"
echo "Building for x86_64-apple-darwin (Intel)..."
cargo build --release --target x86_64-apple-darwin --manifest-path "$CRATE_DIR/Cargo.toml"

echo "Creating .app bundle..."

APP_NAME="Rustitles v${VERSION}.app"
CONTENTS_DIR="$APP_NAME/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

rm -rf Rustitles\ v*.app

mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"

# Universal binary (single .app runs on both Intel and Apple Silicon)
lipo -create -output "$MACOS_DIR/rustitles" \
  "$CRATE_DIR/target/aarch64-apple-darwin/release/rustitles" \
  "$CRATE_DIR/target/x86_64-apple-darwin/release/rustitles"

# Icon: Dock and system use .icns only. Prefer PNG so the correct design is always used.
ICON_TMP=$(mktemp -d)
ICONSET_DIR="$ICON_TMP/rustitles.iconset"
trap "rm -rf '$ICON_TMP'" EXIT

PNG=""
[ -f "$CRATE_DIR/resources/rustitles_mac_icon.png" ] && PNG="$CRATE_DIR/resources/rustitles_mac_icon.png"
[ -z "$PNG" ] && [ -f "$CRATE_DIR/resources/rustitles_icon.png" ] && PNG="$CRATE_DIR/resources/rustitles_icon.png"

if [ -n "$PNG" ]; then
    # Always generate .icns from PNG so Dock shows rustitles_mac_icon (not an old .icns)
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

echo "Build complete!"
echo "App bundle created at: $APP_NAME"
echo ""
echo "To run: open \"$APP_NAME\""
echo "To create a styled DMG (drag-to-Applications window): ./create-dmg.sh"
echo ""
echo "Icon is built as .icns so the Dock shows it correctly. If Dock still shows old icon: quit app, run 'killall Dock', or remove from Dock and drag app back."
