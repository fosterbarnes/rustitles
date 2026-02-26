#!/bin/bash
# Create a styled DMG with a "drag app to Applications" window.
# Run this on macOS after building the .app (./buildMac.sh).
# Optional: add resources/dmg_background.png (500×350 px) for a custom background.

set -e

ROOT_FOLDER="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$ROOT_FOLDER/crate"
cd "$ROOT_FOLDER"

VERSION_TXT="$CRATE_DIR/src/version.txt"
VERSION=$(cat "$VERSION_TXT" | tr -d '\n' | tr -d '\r' | xargs)
APP_NAME="Rustitles v${VERSION}.app"
DMG_NAME="Rustitles-v${VERSION}-macOS.dmg"
VOLUME_NAME="Rustitles"

# ========== LAYOUT (edit these to adjust the DMG window) ==========
# All positions are in pixels. Origin (0,0) is top-left of the screen; the window
# is positioned on the screen at (WIN_X, WIN_Y) and has size (WIN_W, WIN_H).

# Window position on screen when the DMG opens (usually leave as-is)
WIN_X=100
WIN_Y=120

# Window size in pixels (change to match your background image or preference)
WIN_W=500
WIN_H=350

# Icon size in the Finder view (32–128 typical; 128 = large icons)
ICON_SIZE=128

# Label text size under icons (10–16; affects how much vertical space each icon uses)
TEXT_SIZE=12

# Icon positions *inside* the window (relative to window top-left)
# App icon (left side) — drag source
APP_ICON_X=100
APP_ICON_Y=90
# Applications folder (right side) — drop target
APPLICATIONS_ICON_X=300
APPLICATIONS_ICON_Y=90
# Tip: spacing between the two = APPLICATIONS_ICON_X - APP_ICON_X - ~100 (icon width)
# ========== END LAYOUT ==========

if [[ ! -d "$APP_NAME" ]]; then
	echo "Error: $APP_NAME not found. Run ./buildMac.sh first."
	exit 1
fi

echo "Creating DMG: $DMG_NAME (volume: $VOLUME_NAME)"
echo "Window size: ${WIN_W}x${WIN_H} (drag-to-Applications layout)"
echo ""

# Prepare source folder for the DMG (only the .app and optional background)
DMG_SRC=$(mktemp -d -t rustitles_dmg_src.XXXXXXXXXX)
cp -R "$APP_NAME" "$DMG_SRC/"

BACKGROUND_FILE=""
if [[ -f "$CRATE_DIR/resources/dmg_background.png" ]]; then
	mkdir -p "$DMG_SRC/.background"
	cp "$CRATE_DIR/resources/dmg_background.png" "$DMG_SRC/.background/dmg_background.png"
	BACKGROUND_FILE="dmg_background.png"
	echo "Using custom background: crate/resources/dmg_background.png"
fi

# Remove any previous temp DMG
DMG_TEMP="$ROOT_FOLDER/rw.$$.${DMG_NAME}"
DMG_FINAL="$ROOT_FOLDER/$DMG_NAME"
rm -f "$DMG_TEMP"

# Create read-write DMG
echo "Creating disk image..."
hdiutil create -srcfolder "$DMG_SRC" -volname "$VOLUME_NAME" \
	-fs HFS+ -fsargs "-c c=64,a=16,e=16" -format UDRW -size 80m "$DMG_TEMP"
rm -rf "$DMG_SRC"

# Mount it
echo "Mounting..."
DEV=$(hdiutil attach -readwrite -noverify -noautoopen "$DMG_TEMP" | grep -E '^/dev/' | sed 1q | awk '{print $1}')
# Volume is mounted at /Volumes/<volname>
MOUNT_DIR="/Volumes/$VOLUME_NAME"
if [[ ! -d "$MOUNT_DIR" ]]; then
	echo "Error: mount point $MOUNT_DIR not found for $DEV"
	hdiutil detach "$DEV" 2>/dev/null || true
	rm -f "$DMG_TEMP"
	exit 1
fi

# Add Applications symlink
ln -s /Applications "$MOUNT_DIR/Applications"

# Copy background into mounted volume if we have one
if [[ -n "$BACKGROUND_FILE" ]]; then
	mkdir -p "$MOUNT_DIR/.background"
	cp "$CRATE_DIR/resources/dmg_background.png" "$MOUNT_DIR/.background/$BACKGROUND_FILE"
fi

# Give Finder a moment, then run AppleScript to style the window
sleep 2

APPLESCRIPT=$(mktemp -t rustitles_dmg_applescript.XXXXXXXXXX)
cat > "$APPLESCRIPT" << 'ASCRIPT'
on run volumeName
  tell application "Finder"
    tell disk (volumeName as string)
      open
      set theXOrigin to WIN_X
      set theYOrigin to WIN_Y
      set theWidth to WIN_W
      set theHeight to WIN_H
      set theBottomRightX to (theXOrigin + theWidth)
      set theBottomRightY to (theYOrigin + theHeight)

      tell container window
        set current view to icon view
        set toolbar visible to false
        set statusbar visible to false
        set the bounds to {theXOrigin, theYOrigin, theBottomRightX, theBottomRightY}
      end tell

      set opts to the icon view options of container window
      tell opts
        set icon size to ICON_SIZE
        set text size to TEXT_SIZE
        set arrangement to not arranged
        BACKGROUND_LINE
      end tell

      set position of item "APP_NAME_PLACEHOLDER" of container window to {APP_ICON_X, APP_ICON_Y}
      set position of item "Applications" of container window to {APPLICATIONS_ICON_X, APPLICATIONS_ICON_Y}

      close
      open
      delay 1
      tell container window
        set the bounds to {theXOrigin, theYOrigin, theBottomRightX - 10, theBottomRightY - 10}
      end tell
      delay 2
    end tell
  end tell
end run
ASCRIPT

# Substitute layout constants and app name
sed -i '' \
  -e "s/WIN_X/$WIN_X/g" -e "s/WIN_Y/$WIN_Y/g" \
  -e "s/WIN_W/$WIN_W/g" -e "s/WIN_H/$WIN_H/g" \
  -e "s/ICON_SIZE/$ICON_SIZE/g" -e "s/TEXT_SIZE/$TEXT_SIZE/g" \
  -e "s/APP_ICON_X/$APP_ICON_X/g" -e "s/APP_ICON_Y/$APP_ICON_Y/g" \
  -e "s/APPLICATIONS_ICON_X/$APPLICATIONS_ICON_X/g" -e "s/APPLICATIONS_ICON_Y/$APPLICATIONS_ICON_Y/g" \
  -e "s/APP_NAME_PLACEHOLDER/$APP_NAME/g" \
  "$APPLESCRIPT"

if [[ -n "$BACKGROUND_FILE" ]]; then
	sed -i '' "s|BACKGROUND_LINE|set background picture of opts to file \".background:$BACKGROUND_FILE\"|" "$APPLESCRIPT"
else
	sed -i '' 's|BACKGROUND_LINE||' "$APPLESCRIPT"
fi

echo "Applying window layout (AppleScript)..."
osascript "$APPLESCRIPT" "$VOLUME_NAME"
rm -f "$APPLESCRIPT"

# Clean up and unmount
chmod -Rf go-w "$MOUNT_DIR" 2>/dev/null || true
sleep 1
echo "Unmounting..."
hdiutil detach "$DEV"

# Compress to read-only DMG
echo "Compressing..."
rm -f "$DMG_FINAL"
hdiutil convert "$DMG_TEMP" -format UDZO -imagekey zlib-level=9 -o "$DMG_FINAL"
rm -f "$DMG_TEMP"

echo ""
echo "Done. DMG saved to: $DMG_FINAL"
echo "Open it to see the drag-to-Applications window."
