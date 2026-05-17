#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/apps/desktop"

# shellcheck disable=SC1091
source "$ROOT_DIR/scripts/load_release_env.sh"

if [[ -z "${CSC_NAME:-}" && -z "${CSC_LINK:-}" ]]; then
  export CSC_IDENTITY_AUTO_DISCOVERY=false
  echo "[sign] Disabled electron-builder identity auto-discovery because CSC_NAME / CSC_LINK are not set."
fi

cd "$DESKTOP_DIR"
bun run release:prepare:mac
electron-builder --mac dir --arm64 --publish never
