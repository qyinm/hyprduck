const fs = require("node:fs");
const path = require("node:path");

const SUPPORTED_AGENT_DEFINITIONS = [
  {
    id: "codex",
    label: "Codex",
    commands: ["codex"],
    launchArgs: [],
    support: "supported",
  },
  {
    id: "claude_code",
    label: "Claude Code",
    commands: ["claude"],
    launchArgs: [],
    support: "supported",
  },
  {
    id: "pi_agent",
    label: "Pi Agent",
    commands: ["pi-agent"],
    launchArgs: [],
    support: "experimental",
  },
  {
    id: "hermes",
    label: "Hermes",
    commands: ["hermes"],
    launchArgs: [],
    support: "experimental",
  },
];

function listSupportedAgentDefinitions() {
  return SUPPORTED_AGENT_DEFINITIONS.map((definition) => ({ ...definition }));
}

function detectSupportedAgents(options = {}) {
  const pathEnv = options.pathEnv ?? process.env.PATH ?? "";
  const pathEntries = splitPathEntries(pathEnv);
  return SUPPORTED_AGENT_DEFINITIONS.map((definition) =>
    detectAgent(definition, pathEntries),
  );
}

function detectAgent(definition, pathEntries) {
  const resolved = resolveFirstExecutable(definition.commands, pathEntries);
  if (!resolved) {
    return {
      id: definition.id,
      label: definition.label,
      detected: false,
      support: definition.support,
      commands: definition.commands,
      command: null,
      path: null,
      launchArgs: definition.launchArgs,
      confidence: "missing",
      disabledReason: `${definition.label} command was not found on PATH.`,
    };
  }

  return {
    id: definition.id,
    label: definition.label,
    detected: true,
    support: definition.support,
    commands: definition.commands,
    command: resolved.command,
    path: resolved.path,
    launchArgs: definition.launchArgs,
    confidence: resolved.command === definition.commands[0] ? "high" : "medium",
    disabledReason: null,
  };
}

function resolveFirstExecutable(commands, pathEntries) {
  for (const command of commands) {
    const resolvedPath = resolveExecutable(command, pathEntries);
    if (resolvedPath) {
      return { command, path: resolvedPath };
    }
  }
  return null;
}

function resolveExecutable(command, pathEntries) {
  if (!command || command.includes("/") || command.includes("\\")) {
    return null;
  }
  for (const entry of pathEntries) {
    const candidate = path.join(entry, command);
    if (isExecutableFile(candidate)) {
      return candidate;
    }
  }
  return null;
}

function splitPathEntries(pathEnv) {
  return String(pathEnv)
    .split(path.delimiter)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function isExecutableFile(candidate) {
  try {
    fs.accessSync(candidate, fs.constants.X_OK);
    return fs.statSync(candidate).isFile();
  } catch {
    return false;
  }
}

function assertKnownAgentId(agentId) {
  if (!SUPPORTED_AGENT_DEFINITIONS.some((definition) => definition.id === agentId)) {
    throw new Error(`unknown supported agent id: ${agentId}`);
  }
  return agentId;
}

module.exports = {
  SUPPORTED_AGENT_DEFINITIONS,
  assertKnownAgentId,
  detectSupportedAgents,
  listSupportedAgentDefinitions,
};
