#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="${BASH_SOURCE[0]:-$0}"
ROOT_DIR="$(cd "$(dirname "$SCRIPT_PATH")/.." && pwd)"
PRIMARY_ENV_FILE="$ROOT_DIR/.env.release.local"
LEGACY_ENV_FILE="$ROOT_DIR/.env.local.release"

if [[ -f "$PRIMARY_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$PRIMARY_ENV_FILE"
  set +a
elif [[ -f "$LEGACY_ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$LEGACY_ENV_FILE"
  set +a
fi
