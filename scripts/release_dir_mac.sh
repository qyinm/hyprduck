#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/apps/desktop"

# shellcheck disable=SC1091
source "$ROOT_DIR/scripts/load_release_env.sh"

cd "$DESKTOP_DIR"
bun run release:prepare:mac
electron-builder --mac dir --arm64 --publish never
