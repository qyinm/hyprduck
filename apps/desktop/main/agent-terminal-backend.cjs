const AGENT_TERMINAL_BACKEND_VERSION = 1;

class AgentTerminalBackend {
  constructor(name) {
    if (!name) {
      throw new Error("agent terminal backend requires a name");
    }
    this.name = name;
  }

  async createSession() {
    throw new Error(`${this.name} does not implement createSession`);
  }

  async write() {
    throw new Error(`${this.name} does not implement write`);
  }

  async resize() {
    throw new Error(`${this.name} does not implement resize`);
  }

  async kill() {
    throw new Error(`${this.name} does not implement kill`);
  }

  subscribe() {
    throw new Error(`${this.name} does not implement subscribe`);
  }

  snapshotStatus() {
    throw new Error(`${this.name} does not implement snapshotStatus`);
  }
}

class DisabledAgentTerminalBackend extends AgentTerminalBackend {
  constructor(options = {}) {
    super("disabled");
    this.reason =
      options.reason ??
      "Native Ghostty backend has not passed the Etyma spike gate.";
  }

  async createSession() {
    return {
      backend: this.name,
      status: "unavailable",
      reason: this.reason,
      fallback: "external_ghostty",
    };
  }

  async write() {
    throw new Error(this.reason);
  }

  async resize() {
    return { status: "ignored", reason: this.reason };
  }

  async kill() {
    return { status: "ignored", reason: this.reason };
  }

  subscribe() {
    return () => {};
  }

  snapshotStatus() {
    return {
      backend: this.name,
      available: false,
      reason: this.reason,
      fallback: "external_ghostty",
      version: AGENT_TERMINAL_BACKEND_VERSION,
    };
  }
}

function createDefaultAgentTerminalBackend(options = {}) {
  const ptyProbe = tryCreatePtyBackend(options);
  if (ptyProbe.backend) {
    return ptyProbe.backend;
  }
  return new DisabledAgentTerminalBackend({
    reason:
      ptyProbe.reason ??
      "PTY backend is unavailable; use the external Ghostty fallback.",
  });
}

function tryCreatePtyBackend(options = {}) {
  try {
    const {
      tryCreatePtyAgentTerminalBackend,
    } = require("./agent-terminal-pty.cjs");
    return tryCreatePtyAgentTerminalBackend(options);
  } catch (error) {
    return {
      backend: null,
      reason: `PTY backend unavailable: ${error.message}`,
    };
  }
}

function assertAgentTerminalBackend(candidate) {
  const requiredMethods = [
    "createSession",
    "write",
    "resize",
    "kill",
    "subscribe",
    "snapshotStatus",
  ];
  const missing = requiredMethods.filter(
    (method) => typeof candidate?.[method] !== "function",
  );
  if (missing.length > 0) {
    throw new Error(`agent terminal backend missing methods: ${missing.join(", ")}`);
  }
  return candidate;
}

module.exports = {
  AGENT_TERMINAL_BACKEND_VERSION,
  AgentTerminalBackend,
  DisabledAgentTerminalBackend,
  assertAgentTerminalBackend,
  createDefaultAgentTerminalBackend,
};
