#!/bin/zsh
set -euo pipefail

MODE="${1:-debug}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BIN_DIR="${REPO_ROOT}/apps/desktop/src-tauri/binaries"
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"

if [[ -z "${HOST_TRIPLE}" ]]; then
  echo "failed to determine rust host triple" >&2
  exit 1
fi

mkdir -p "${BIN_DIR}"

pushd "${REPO_ROOT}" >/dev/null
if [[ "${MODE}" == "release" ]]; then
  cargo build -p duckdocs-engine --release
  SOURCE_BIN="${REPO_ROOT}/target/release/duckdocs-engine"
else
  cargo build -p duckdocs-engine
  SOURCE_BIN="${REPO_ROOT}/target/debug/duckdocs-engine"
fi
popd >/dev/null

TARGET_BIN="${BIN_DIR}/duckdocs-engine-${HOST_TRIPLE}"
cp "${SOURCE_BIN}" "${TARGET_BIN}"
chmod +x "${TARGET_BIN}"

echo "synced ${SOURCE_BIN} -> ${TARGET_BIN}"
