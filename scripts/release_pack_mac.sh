#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/apps/desktop"

find_codesign_identity() {
  local preferred_name="${1:-}"
  local identity

  if [[ -n "$preferred_name" ]]; then
    while IFS= read -r identity; do
      if [[ "$identity" == "Developer ID Application: $preferred_name" || "$identity" == "$preferred_name" ]]; then
        printf '%s\n' "$identity"
        return 0
      fi
    done < <(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(.*\)"/\1/p')
  fi

  while IFS= read -r identity; do
    if [[ "$identity" == Developer\ ID\ Application:* ]]; then
      printf '%s\n' "$identity"
      return 0
    fi
  done < <(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(.*\)"/\1/p')

  return 1
}

run_with_timeout() {
  local timeout_seconds="$1"
  shift
  local pid
  local elapsed=0

  "$@" &
  pid="$!"
  while kill -0 "$pid" 2>/dev/null; do
    if (( elapsed >= timeout_seconds )); then
      echo "[timeout] Command exceeded ${timeout_seconds}s: $*" >&2
      kill "$pid" 2>/dev/null || true
      sleep 2
      kill -9 "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      return 124
    fi
    sleep 5
    elapsed=$((elapsed + 5))
  done

  wait "$pid"
}

write_update_manifest() {
  local version="$1"
  local zip_path="$2"
  local dmg_path="$3"
  local update_path="$4"
  local zip_sha512
  local dmg_sha512
  local zip_size
  local dmg_size
  local release_date

  zip_sha512="$(openssl dgst -sha512 -binary "$zip_path" | openssl base64 -A)"
  dmg_sha512="$(openssl dgst -sha512 -binary "$dmg_path" | openssl base64 -A)"
  zip_size="$(stat -f%z "$zip_path")"
  dmg_size="$(stat -f%z "$dmg_path")"
  release_date="$(date -u +"%Y-%m-%dT%H:%M:%S.000Z")"

  cat > "$update_path" <<EOF
version: $version
files:
  - url: Etyma-${version}-mac-arm64.zip
    sha512: $zip_sha512
    size: $zip_size
  - url: Etyma-${version}-mac-arm64.dmg
    sha512: $dmg_sha512
    size: $dmg_size
path: Etyma-${version}-mac-arm64.zip
sha512: $zip_sha512
releaseDate: '$release_date'
EOF
}

# shellcheck disable=SC1091
source "$ROOT_DIR/scripts/load_release_env.sh"

if [[ -z "${CSC_NAME:-}" && -z "${CSC_LINK:-}" ]]; then
  export CSC_IDENTITY_AUTO_DISCOVERY=false
  echo "[sign] Disabled electron-builder identity auto-discovery because CSC_NAME / CSC_LINK are not set."
fi

cd "$DESKTOP_DIR"
bun run release:prepare:mac
electron-builder --mac dir --arm64 --publish never

VERSION="$(node -p "require('./package.json').version")"
APP_PATH="$DESKTOP_DIR/release/electron/mac-arm64/Etyma.app"
DMG_PATH="$DESKTOP_DIR/release/electron/Etyma-${VERSION}-mac-arm64.dmg"
ZIP_PATH="$DESKTOP_DIR/release/electron/Etyma-${VERSION}-mac-arm64.zip"
UPDATE_PATH="$DESKTOP_DIR/release/electron/latest-mac.yml"
DMG_NOTARIZED_MARKER="$DESKTOP_DIR/release/electron/.dmg-notarized"
DMG_STAGING_DIR="$DESKTOP_DIR/release/electron/dmg-staging"

if [[ ! -d "$APP_PATH" ]]; then
  echo "Expected app was not created: $APP_PATH" >&2
  exit 1
fi

xattr -cr "$APP_PATH"
codesign --verify --deep --strict --verbose=4 "$APP_PATH"
for helper in "$APP_PATH"/Contents/Resources/app.asar.unpacked/node_modules/node-pty/prebuilds/darwin-*/spawn-helper; do
  test -x "$helper"
  codesign --verify --strict --verbose=4 "$helper"
done

rm -f "$DMG_PATH" "$ZIP_PATH" "$UPDATE_PATH" "$DMG_NOTARIZED_MARKER"
rm -rf "$DMG_STAGING_DIR"

ditto -c -k --sequesterRsrc --keepParent --noextattr --noqtn "$APP_PATH" "$ZIP_PATH"

mkdir -p "$DMG_STAGING_DIR"
ditto --noextattr --noqtn "$APP_PATH" "$DMG_STAGING_DIR/Etyma.app"
ln -s /Applications "$DMG_STAGING_DIR/Applications"
xattr -cr "$DMG_STAGING_DIR"
hdiutil create \
  -volname "Etyma ${VERSION}-arm64" \
  -srcfolder "$DMG_STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

if [[ -n "${APPLE_ID:-}" && -n "${APPLE_APP_SPECIFIC_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  if [[ ! -f "$DMG_PATH" ]]; then
    echo "Expected DMG was not created: $DMG_PATH" >&2
    exit 1
  fi

  if [[ -n "${CSC_NAME:-}" ]]; then
    echo "[sign] Signing $DMG_PATH"
    if DMG_SIGN_IDENTITY="$(find_codesign_identity "$CSC_NAME")"; then
      codesign --force --sign "$DMG_SIGN_IDENTITY" "$DMG_PATH"
    else
      echo "[sign] Skipping DMG signing because no Developer ID Application identity is available in the active keychain."
    fi
  else
    echo "[sign] Skipping DMG signing because CSC_NAME is not set."
  fi

  echo "[notarize] Submitting $DMG_PATH"
  if run_with_timeout 900 xcrun notarytool submit "$DMG_PATH" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait; then
    echo "[notarize] Stapling $DMG_PATH"
    xcrun stapler staple "$DMG_PATH"
    touch "$DMG_NOTARIZED_MARKER"
  else
    echo "[notarize] WARNING: DMG notarization did not finish within the timeout. The app bundle remains notarized; publishing will continue with an unstapled DMG." >&2
  fi
else
  echo "[notarize] Skipping DMG notarization because APPLE_ID / APPLE_APP_SPECIFIC_PASSWORD / APPLE_TEAM_ID are not all set."
fi

write_update_manifest "$VERSION" "$ZIP_PATH" "$DMG_PATH" "$UPDATE_PATH"
