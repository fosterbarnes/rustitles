#!/usr/bin/env bash
set -euo pipefail

ROOT_FOLDER="$(cd "$(dirname "$0")/.." && pwd)"
CRATE_DIR="$ROOT_FOLDER/crate"
VERSION_FILE="$ROOT_FOLDER/.version/version"
PUBLISH_DIR="$ROOT_FOLDER/publish"
LINUXDEPLOY_VERSION="1-alpha-20251107-1"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/$LINUXDEPLOY_VERSION/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_SHA256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
export PATH="$HOME/.cargo/bin:$PATH"

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Error: required command not found: $1" >&2
        exit 1
    fi
}

if [ "$(uname -s)" != "Linux" ] || [ ! -f /etc/debian_version ]; then
    echo "Error: buildDebian.sh requires a Debian Linux host." >&2
    exit 1
fi

for command_name in cargo curl dpkg dpkg-deb dpkg-shlibdeps pkg-config sha256sum uname; do
    require_command "$command_name"
done

if ! pkg-config --exists openssl; then
    echo "Error: OpenSSL development metadata is required. Install pkg-config and libssl-dev." >&2
    exit 1
fi

if [ "$(dpkg --print-architecture)" != "amd64" ] || [ "$(uname -m)" != "x86_64" ]; then
    echo "Error: buildDebian.sh requires a native amd64 host." >&2
    exit 1
fi

if ! CARGO_DEB_VERSION_OUTPUT=$(cargo deb --version 2>/dev/null); then
    echo "Error: cargo-deb is required. Install cargo-deb 3.7.0 with cargo install cargo-deb --version 3.7.0 --locked." >&2
    exit 1
fi
CARGO_DEB_VERSION=$(printf '%s\n' "$CARGO_DEB_VERSION_OUTPUT" | sed -n 's/.* \([0-9][0-9.]*\)$/\1/p')
if [ "$CARGO_DEB_VERSION" != "3.7.0" ]; then
    echo "Error: cargo-deb 3.7.0 is required; found ${CARGO_DEB_VERSION:-unknown}." >&2
    exit 1
fi

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
' "$CARGO_TOML" | tr -d '\r' | xargs)
CONFIG_RS="$CRATE_DIR/src/config.rs"
CONFIG_VERSION=$(sed -n 's/^pub const APP_VERSION: &str = "\([^"]*\)";/\1/p' "$CONFIG_RS" | tr -d '\r' | xargs)
if [ "$CRATE_VERSION" != "$VERSION" ] || [ "$CARGO_VERSION" != "$VERSION" ] || [ "$CONFIG_VERSION" != "$VERSION" ]; then
    echo "Error: version consumers do not match $VERSION" >&2
    exit 1
fi

cd "$ROOT_FOLDER"
mkdir -p "$PUBLISH_DIR"
rm -f "$PUBLISH_DIR/rustitles.AppImage" "$PUBLISH_DIR/rustitles.deb"
rm -f "$ROOT_FOLDER/target/release/rustitles"

echo "Building Rustitles v${VERSION} for Debian amd64..."
cargo build --release --manifest-path "$CRATE_DIR/Cargo.toml"
if [ ! -x "$ROOT_FOLDER/target/release/rustitles" ]; then
    echo "Error: release binary was not produced." >&2
    exit 1
fi

echo "Building Debian package..."
cargo deb --manifest-path "$CRATE_DIR/Cargo.toml" --no-build --output "$PUBLISH_DIR/rustitles.deb"
if [ ! -f "$PUBLISH_DIR/rustitles.deb" ]; then
    echo "Error: Debian package was not produced." >&2
    exit 1
fi

TOOL_DIR=$(mktemp -d "${TMPDIR:-/tmp}/rustitles-debian.XXXXXX")
trap 'rm -rf "$TOOL_DIR"' EXIT
LINUXDEPLOY_PATH="$TOOL_DIR/linuxdeploy-x86_64.AppImage"

echo "Downloading pinned linuxdeploy ${LINUXDEPLOY_VERSION}..."
curl --fail --location --silent --show-error --retry 3 "$LINUXDEPLOY_URL" -o "$LINUXDEPLOY_PATH"
printf '%s  %s\n' "$LINUXDEPLOY_SHA256" "$LINUXDEPLOY_PATH" | sha256sum --check --status -
chmod 755 "$LINUXDEPLOY_PATH"

APPDIR="$TOOL_DIR/AppDir"
DESKTOP_PATH="$TOOL_DIR/rustitles.desktop"
ICON_PATH="$TOOL_DIR/rustitles.png"
mkdir -p "$APPDIR"
sed 's/\r$//' "$CRATE_DIR/resources/rustitles.desktop" > "$DESKTOP_PATH"
cp -f "$CRATE_DIR/resources/rustitles_icon_256.png" "$ICON_PATH"

echo "Building AppImage..."
APPIMAGE_EXTRACT_AND_RUN=1 \
LDAI_NO_APPSTREAM=1 \
LDAI_OUTPUT="$PUBLISH_DIR/rustitles.AppImage" \
LINUXDEPLOY_OUTPUT_APP_NAME=rustitles \
LINUXDEPLOY_OUTPUT_VERSION="$VERSION" \
"$LINUXDEPLOY_PATH" \
    --appdir "$APPDIR" \
    --executable "$ROOT_FOLDER/target/release/rustitles" \
    --desktop-file "$DESKTOP_PATH" \
    --icon-file "$ICON_PATH" \
    --output appimage

if [ ! -f "$PUBLISH_DIR/rustitles.AppImage" ]; then
    echo "Error: AppImage was not produced." >&2
    exit 1
fi
chmod 755 "$PUBLISH_DIR/rustitles.AppImage"

echo "Release assets staged in $PUBLISH_DIR"
