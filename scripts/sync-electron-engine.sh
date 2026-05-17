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
  cargo build -p hyprduck-engine -p hyprduck-cli --release
  PROFILE_DIR="${REPO_ROOT}/target/release"
else
  cargo build -p hyprduck-engine -p hyprduck-cli
  PROFILE_DIR="${REPO_ROOT}/target/debug"
fi
popd >/dev/null

ENGINE_TARGET_BIN="${BIN_DIR}/hyprduck-engine-${HOST_TRIPLE}"
CLI_TARGET_BIN="${BIN_DIR}/hyprduck-${HOST_TRIPLE}"

cp "${PROFILE_DIR}/hyprduck-engine" "${ENGINE_TARGET_BIN}"
cp "${PROFILE_DIR}/hyprduck" "${CLI_TARGET_BIN}"
chmod +x "${ENGINE_TARGET_BIN}" "${CLI_TARGET_BIN}"

echo "synced engine ${PROFILE_DIR}/hyprduck-engine -> ${ENGINE_TARGET_BIN}"
echo "synced cli ${PROFILE_DIR}/hyprduck -> ${CLI_TARGET_BIN}"
