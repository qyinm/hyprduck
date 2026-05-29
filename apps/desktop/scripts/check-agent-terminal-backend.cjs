#!/usr/bin/env node

const {
  assertAgentTerminalBackend,
  createDefaultAgentTerminalBackend,
} = require("../main/agent-terminal-backend.cjs");

function checkDefaultBackend() {
  const backend = assertAgentTerminalBackend(createDefaultAgentTerminalBackend());
  const status = backend.snapshotStatus();
  if (status.backend !== "disabled" || status.fallback !== "external_ghostty") {
    throw new Error(`unexpected default backend status: ${JSON.stringify(status)}`);
  }
  return status;
}

async function checkOptionalGhosttyModule() {
  const moduleName = process.env.HYPRDUCK_GHOSTTY_BACKEND_MODULE;
  if (!moduleName) {
    return {
      checked: false,
      reason: "Set HYPRDUCK_GHOSTTY_BACKEND_MODULE to test a native Ghostty backend module.",
    };
  }

  const loaded = require(moduleName);
  const factory =
    typeof loaded.createAgentTerminalBackend === "function"
      ? loaded.createAgentTerminalBackend
      : loaded.default;
  if (typeof factory !== "function") {
    throw new Error(
      `${moduleName} must export createAgentTerminalBackend() or a default factory`,
    );
  }
  const backend = assertAgentTerminalBackend(await factory());
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
