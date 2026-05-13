#!/bin/zsh
set -euo pipefail

MODE="${1:-debug}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BIN_DIR="${REPO_ROOT}/apps/desktop/resources/binaries"
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"

if [[ -z "${HOST_TRIPLE}" ]]; then
  echo "failed to determine rust host triple" >&2
  exit 1
fi

mkdir -p "${BIN_DIR}"

pushd "${REPO_ROOT}" >/dev/null
if [[ "${MODE}" == "release" ]]; then
  cargo build -p hyprduck-engine --release
  SOURCE_BIN="${REPO_ROOT}/target/release/hyprduck-engine"
else
  cargo build -p hyprduck-engine
  SOURCE_BIN="${REPO_ROOT}/target/debug/hyprduck-engine"
fi
popd >/dev/null

TARGET_BIN="${BIN_DIR}/hyprduck-engine-${HOST_TRIPLE}"
cp "${SOURCE_BIN}" "${TARGET_BIN}"
chmod +x "${TARGET_BIN}"

echo "synced engine ${SOURCE_BIN} -> ${TARGET_BIN}"
