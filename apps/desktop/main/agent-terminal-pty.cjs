const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  AgentTerminalBackend,
  AGENT_TERMINAL_BACKEND_VERSION,
} = require("./agent-terminal-backend.cjs");

class PtyAgentTerminalBackend extends AgentTerminalBackend {
  constructor(options = {}) {
    super("node_pty");
    this.pty = options.pty;
    if (!options.pty) {
      prepareNodePtyRuntime();
    }
    this.env = options.env ?? process.env;
    this.cwd = options.cwd ?? resolveWorkingDirectory(this.env);
    this.sessions = new Map();
  }

  async createSession(args) {
    const command = args.agent.path ?? args.agent.command;
    if (!command) {
      throw new Error(`${args.agent.label} does not have a launch command`);
    }

    const ptyProcess = this.pty.spawn(command, args.agent.launchArgs ?? [], {
      name: "xterm-256color",
      cols: args.cols,
      rows: args.rows,
      cwd: this.cwd,
      env: {
        ...this.env,
        TERM: "xterm-256color",
        COLORTERM: "truecolor",
        HYPRDUCK_AGENT_TERMINAL: "1",
        HYPRDUCK_WORKSPACE_ID: args.handoff.workspace.workspaceId,
        HYPRDUCK_PROJECT_ID: args.handoff.workspace.projectId ?? "",
        HYPRDUCK_NODE_ID: args.handoff.workspace.nodeId ?? "",
      },
    });

    const record = {
      id: args.sessionId,
      ptyProcess,
      listeners: new Set(),
      buffer: "",
      exitEvent: null,
      closed: false,
    };
    this.sessions.set(args.sessionId, record);

    attachDataHandler(ptyProcess, (data) => {
      this.emit(args.sessionId, { type: "data", data: String(data) });
    });
    attachExitHandler(ptyProcess, (exit) => {
      record.closed = true;
      record.exitEvent = {
        type: "exit",
        exitCode: normalizeExitCode(exit),
        signal: normalizeExitSignal(exit),
      };
      this.emit(args.sessionId, record.exitEvent);
    });

    return {
      id: args.sessionId,
      backend: this.name,
      status: "running",
      handoffStatus: "attached",
      pid: ptyProcess.pid ?? null,
      cwd: this.cwd,
      version: AGENT_TERMINAL_BACKEND_VERSION,
    };
  }

  async write(sessionId, input) {
    const record = this.requireSession(sessionId);
    if (record.closed) {
      return { status: "blocked", reason: "terminal session is closed" };
    }
    record.ptyProcess.write(input);
    return { status: "written" };
  }

  async resize(sessionId, dimensions) {
    const record = this.requireSession(sessionId);
    if (record.closed) {
      return { status: "ignored", reason: "terminal session is closed" };
    }
    record.ptyProcess.resize(dimensions.cols, dimensions.rows);
    return { status: "resized" };
  }

  async kill(sessionId) {
    const record = this.requireSession(sessionId);
    if (!record.closed) {
      record.ptyProcess.kill();
    }
    this.sessions.delete(sessionId);
    return { status: "closed" };
  }

  subscribe(sessionId, listener) {
    const record = this.requireSession(sessionId);
    record.listeners.add(listener);
    if (record.buffer) {
      listener({ sessionId, type: "data", data: record.buffer });
    }
    if (record.exitEvent) {
      listener({ sessionId, ...record.exitEvent });
    }
    return () => record.listeners.delete(listener);
  }

  snapshotStatus() {
    return {
      backend: this.name,
      available: true,
      version: AGENT_TERMINAL_BACKEND_VERSION,
      cwd: this.cwd,
    };
  }

  requireSession(sessionId) {
    const record = this.sessions.get(sessionId);
    if (!record) {
      throw new Error(`unknown pty agent terminal session: ${sessionId}`);
    }
    return record;
  }

  emit(sessionId, event) {
    const record = this.sessions.get(sessionId);
    if (!record) {
      return;
    }
    if (event.type === "data") {
      record.buffer += event.data;
      if (record.buffer.length > 300_000) {
        record.buffer = record.buffer.slice(-300_000);
      }
    }
    for (const listener of record.listeners) {
      listener({ sessionId, ...event });
    }
  }
}

function createPtyAgentTerminalBackend(options = {}) {
  const pty = options.pty ?? require("node-pty");
  if (!options.pty) {
    prepareNodePtyRuntime();
  }
  return new PtyAgentTerminalBackend({ ...options, pty });
}

function tryCreatePtyAgentTerminalBackend(options = {}) {
  try {
    return {
      backend: createPtyAgentTerminalBackend(options),
      reason: null,
    };
  } catch (error) {
    return {
      backend: null,
      reason: `PTY backend unavailable: ${error.message}`,
    };
  }
}

function resolveWorkingDirectory(env) {
  const configured = env.HYPRDUCK_AGENT_TERMINAL_CWD;
  if (configured && isDirectory(configured)) {
    return configured;
  }
  return env.HOME && isDirectory(env.HOME) ? env.HOME : os.homedir();
}

function isDirectory(candidate) {
  try {
    return fs.statSync(candidate).isDirectory();
  } catch {
    return false;
  }
}

function attachDataHandler(ptyProcess, handler) {
  if (typeof ptyProcess.onData === "function") {
    ptyProcess.onData(handler);
    return;
  }
  ptyProcess.on("data", handler);
}

function prepareNodePtyRuntime() {
  const helperPath = resolveNodePtySpawnHelper();
  if (!helperPath || process.platform !== "darwin") {
    return;
  }
  if (isPackagedAppResource(helperPath)) {
    return;
  }
  try {
    const mode = fs.statSync(helperPath).mode;
    if ((mode & 0o111) === 0) {
      fs.chmodSync(helperPath, 0o755);
    }
  } catch {
    // node-pty will surface the concrete launch failure if the helper is unusable.
  }
}

function isPackagedAppResource(candidate) {
  if (!process.resourcesPath) {
    return false;
  }
  const unpackedResourceDir = path.join(
    process.resourcesPath,
    "app.asar.unpacked",
  );
  return path.resolve(candidate).startsWith(path.resolve(unpackedResourceDir));
}

function resolveNodePtySpawnHelper() {
  try {
    const packageJsonPath = require.resolve("node-pty/package.json");
    return path.join(
      path.dirname(packageJsonPath),
      "prebuilds",
      `${process.platform}-${process.arch}`,
      "spawn-helper",
    );
  } catch {
    return null;
  }
}

function attachExitHandler(ptyProcess, handler) {
  if (typeof ptyProcess.onExit === "function") {
    ptyProcess.onExit(handler);
    return;
  }
  ptyProcess.on("exit", (exitCode, signal) => handler({ exitCode, signal }));
}

function normalizeExitCode(exit) {
  if (typeof exit?.exitCode === "number") return exit.exitCode;
  if (typeof exit === "number") return exit;
  return null;
}

function normalizeExitSignal(exit) {
  if (typeof exit?.signal === "number" || typeof exit?.signal === "string") {
    return exit.signal;
  }
  return null;
}

module.exports = {
  PtyAgentTerminalBackend,
  createPtyAgentTerminalBackend,
  isPackagedAppResource,
  prepareNodePtyRuntime,
  tryCreatePtyAgentTerminalBackend,
};
