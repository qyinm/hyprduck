#!/usr/bin/env node

const {
  assertAgentTerminalBackend,
  createDefaultAgentTerminalBackend,
} = require("../main/agent-terminal-backend.cjs");
const {
  createGhosttyNativeBackendFromEnv,
} = require("../main/agent-terminal-ghostty.cjs");

function checkDefaultBackend() {
  const backend = assertAgentTerminalBackend(createDefaultAgentTerminalBackend());
  const status = backend.snapshotStatus();
  if (status.backend !== "disabled" || status.fallback !== "external_ghostty") {
    throw new Error(`unexpected default backend status: ${JSON.stringify(status)}`);
  }
  return status;
}

async function checkOptionalGhosttyModule() {
  const result = await createGhosttyNativeBackendFromEnv();
  if (!result.backend) {
    return { checked: result.enabled, reason: result.reason };
  }
  const backend = assertAgentTerminalBackend(result.backend);
  return {
    checked: true,
    status: backend.snapshotStatus(),
  };
}

async function main() {
  const defaultStatus = checkDefaultBackend();
  const ghosttyProbe = await checkOptionalGhosttyModule();
  console.log(
    JSON.stringify(
      {
        ok: true,
        defaultStatus,
        ghosttyProbe,
      },
      null,
      2,
    ),
  );
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
