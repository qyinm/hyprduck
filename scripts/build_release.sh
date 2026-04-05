#!/bin/bash
# DuckDocs Release Build Script
# Usage: ./scripts/build_release.sh

set -e

# Configuration
PROJECT_NAME="DuckDocs"
SCHEME="DuckDocs"
CONFIGURATION="Release"
BUILD_DIR="./build"
ARCHIVE_PATH="$BUILD_DIR/$PROJECT_NAME.xcarchive"
EXPORT_PATH="$BUILD_DIR/export"
DMG_PATH="$BUILD_DIR/$PROJECT_NAME.dmg"
PROJECT_PATH="./apps/macos/$PROJECT_NAME.xcodeproj"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== DuckDocs Release Build ===${NC}"
echo ""

# Clean build directory
echo -e "${YELLOW}Cleaning build directory...${NC}"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# Build archive
echo -e "${YELLOW}Building archive...${NC}"
xcodebuild -project "$PROJECT_PATH" \
    -scheme "$SCHEME" \
    -configuration "$CONFIGURATION" \
    -archivePath "$ARCHIVE_PATH" \
    archive

# Export app
echo -e "${YELLOW}Exporting app...${NC}"

# Create export options plist
cat > "$BUILD_DIR/ExportOptions.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>developer-id</string>
    <key>teamID</key>
    <string>MCP4D3M7XK</string>
    <key>signingStyle</key>
    <string>automatic</string>
</dict>
</plist>
EOF

xcodebuild -exportArchive \
    -archivePath "$ARCHIVE_PATH" \
    -exportPath "$EXPORT_PATH" \
    -exportOptionsPlist "$BUILD_DIR/ExportOptions.plist"

# Check if app exists
APP_PATH="$EXPORT_PATH/$PROJECT_NAME.app"
if [ ! -d "$APP_PATH" ]; then
    echo -e "${RED}Error: App not found at $APP_PATH${NC}"
    exit 1
fi

echo -e "${GREEN}App exported to: $APP_PATH${NC}"

# Notarize (optional - requires Apple ID credentials)
echo ""
echo -e "${YELLOW}Notarization (optional):${NC}"
echo "To notarize, run:"
echo "  xcrun notarytool submit \"$APP_PATH\" --apple-id YOUR_APPLE_ID --team-id MCP4D3M7XK --wait"
echo "  xcrun stapler staple \"$APP_PATH\""
echo ""

# Create DMG
echo -e "${YELLOW}Creating DMG...${NC}"

# Create temporary DMG folder
DMG_TEMP="$BUILD_DIR/dmg_temp"
mkdir -p "$DMG_TEMP"

# Copy app to temp folder
cp -R "$APP_PATH" "$DMG_TEMP/"

# Create Applications symlink
ln -s /Applications "$DMG_TEMP/Applications"

# Create DMG
hdiutil create -volname "$PROJECT_NAME" \
    -srcfolder "$DMG_TEMP" \
    -ov -format UDZO \
    "$DMG_PATH"

# Cleanup
rm -rf "$DMG_TEMP"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo ""
echo "Output files:"
echo "  Archive: $ARCHIVE_PATH"
echo "  App:     $APP_PATH"
echo "  DMG:     $DMG_PATH"
echo ""
echo -e "${YELLOW}Note: For distribution, you should notarize the app first.${NC}"
