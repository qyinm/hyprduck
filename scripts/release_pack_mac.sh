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

# shellcheck disable=SC1091
source "$ROOT_DIR/scripts/load_release_env.sh"

cd "$DESKTOP_DIR"
bun run release:prepare:mac
electron-builder --mac dmg zip --arm64 --publish never

VERSION="$(node -p "require('./package.json').version")"
DMG_PATH="$DESKTOP_DIR/release/electron/HyprDuck-${VERSION}-mac-arm64.dmg"

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
  xcrun notarytool submit "$DMG_PATH" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait

  echo "[notarize] Stapling $DMG_PATH"
  xcrun stapler staple "$DMG_PATH"
else
  echo "[notarize] Skipping DMG notarization because APPLE_ID / APPLE_APP_SPECIFIC_PASSWORD / APPLE_TEAM_ID are not all set."
fi
