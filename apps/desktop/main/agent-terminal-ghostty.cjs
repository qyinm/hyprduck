const { assertAgentTerminalBackend } = require("./agent-terminal-backend.cjs");

function ghosttyNativeBackendEnabled(env = process.env) {
  return env.HYPRDUCK_AGENT_TERMINAL_BACKEND === "ghostty-native";
}

async function createGhosttyNativeBackendFromEnv(env = process.env) {
  const moduleName = env.HYPRDUCK_GHOSTTY_BACKEND_MODULE;
  if (!ghosttyNativeBackendEnabled(env)) {
    return {
      enabled: false,
      backend: null,
      reason: "HYPRDUCK_AGENT_TERMINAL_BACKEND is not ghostty-native.",
    };
  }
  if (!moduleName) {
    return {
      enabled: true,
      backend: null,
      reason: "HYPRDUCK_GHOSTTY_BACKEND_MODULE is required for the native spike.",
    };
  }

  try {
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

    return {
      enabled: true,
      backend: assertAgentTerminalBackend(await factory()),
      reason: null,
    };
  } catch (error) {
    return {
      enabled: true,
      backend: null,
      reason: `Ghostty native backend load failed: ${error.message}`,
    };
  }
}

module.exports = {
  createGhosttyNativeBackendFromEnv,
  ghosttyNativeBackendEnabled,
};
