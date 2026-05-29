const { randomUUID } = require("node:crypto");
const {
  assertKnownAgentId,
  detectSupportedAgents,
} = require("./agent-detection.cjs");
const { createDefaultAgentTerminalBackend } = require("./agent-terminal-backend.cjs");
const {
  assertContextHandoffReady,
  createAgentContextHandoff,
} = require("./agent-terminal-context.cjs");
const {
  assertExternalFallbackReady,
  createExternalGhosttyFallback,
} = require("./agent-terminal-fallbacks.cjs");
const fs = require("node:fs");

const FORBIDDEN_COMMAND_FIELDS = new Set([
  "command",
  "commands",
  "customCommand",
  "shell",
  "shellCommand",
  "executable",
]);

class AgentTerminalSessionManager {
  constructor(options = {}) {
    this.backend = options.backend ?? createDefaultAgentTerminalBackend();
    this.getWorkspaceState = options.getWorkspaceState ?? (() => ({}));
    this.onEvent = options.onEvent ?? (() => {});
    this.sessions = new Map();
    this.outputLimit = options.outputLimit ?? 300_000;
  }

  listAgents() {
    const shell = resolveDefaultShell(process.env);
    return {
      agents: detectSupportedAgents(),
      shell: {
        available: Boolean(shell),
        label: shell ? "New Terminal" : null,
        command: shell?.command ?? null,
        path: shell?.path ?? null,
        reason: shell ? null : "No executable default shell was found.",
      },
    };
  }

  async createSession(args = {}) {
    rejectCommandPayload(args);
    const kind = args.kind ?? args.type ?? "agent";
    const agent =
      kind === "shell"
        ? createShellTerminalAgent(process.env)
        : resolveDetectedAgent(args.agentId ?? args.agent_id);

    const workspaceState = this.getWorkspaceState();
    const handoff = assertContextHandoffReady(
      createAgentContextHandoff({
        workspaceId:
          args.workspaceId ?? args.workspace_id ?? workspaceState.workspaceId ?? "default",
        projectId: args.projectId ?? args.project_id ?? workspaceState.projectId ?? null,
        nodeId: args.nodeId ?? args.node_id ?? null,
        sourceId: workspaceState.sourceId ?? null,
        contextScope: args.contextScope ?? args.context_scope ?? "workspace",
      }),
    );
    const sessionId = randomUUID();
    const backendSession = await this.backend.createSession({
      sessionId,
      agent,
      handoff,
      cols: normalizeDimension(args.cols, 120),
      rows: normalizeDimension(args.rows, 36),
    });
    const fallback = assertExternalFallbackReady(
      createExternalGhosttyFallback(agent, handoff),
    );
    const session = {
      id: sessionId,
      backendSessionId: backendSession.id ?? sessionId,
      agent,
      handoff,
      handoffState: resolveHandoffState(backendSession),
      backend: backendSession,
      fallback,
      status: resolveSessionStatus(backendSession),
      output: "",
      outputSequence: 0,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    this.sessions.set(sessionId, session);
    if (session.status === "running") {
      session.unsubscribe = this.backend.subscribe(
        session.backendSessionId,
        (event) => this.handleBackendEvent(sessionId, event),
      );
    }
    return serializeSession(session);
  }

  snapshotSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    return serializeSession(session);
  }

  async writeSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    const input = String(args.input ?? "");
    if (!input) {
      return { status: "ignored", reason: "empty input" };
    }
    if (session.status === "closed") {
      return { status: "blocked", reason: "terminal session is closed" };
    }
    if (session.handoffState !== "writable") {
      return {
        status: "blocked",
        reason: "Agent context handoff must be attached before terminal input is accepted.",
        handoffState: session.handoffState,
      };
    }
    return this.backend.write(session.backendSessionId, input);
  }

  async resizeSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    if (session.status === "closed") {
      return { status: "ignored", reason: "terminal session is closed" };
    }
    return this.backend.resize(session.backendSessionId, {
      cols: normalizeDimension(args.cols, 120),
      rows: normalizeDimension(args.rows, 36),
    });
  }

  async killSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    const result = await this.backend.kill(session.backendSessionId);
    if (typeof session.unsubscribe === "function") {
      session.unsubscribe();
      session.unsubscribe = null;
    }
    session.status = "closed";
    session.updatedAt = new Date().toISOString();
    this.publishSessionUpdate(session, "session_closed");
    return result;
  }

  requireSession(sessionId) {
    if (!sessionId || !this.sessions.has(sessionId)) {
      throw new Error(`unknown agent terminal session: ${sessionId ?? "(missing)"}`);
    }
    return this.sessions.get(sessionId);
  }

  handleBackendEvent(sessionId, event) {
    const session = this.sessions.get(sessionId);
    if (!session) {
      return;
    }
    if (event.type === "data") {
      session.output += event.data;
      if (session.output.length > this.outputLimit) {
        session.output = session.output.slice(-this.outputLimit);
      }
      session.outputSequence += 1;
    }
    if (event.type === "exit") {
      session.status = "closed";
      session.backend.status = "exited";
      session.backend.exitCode = event.exitCode ?? null;
      session.backend.signal = event.signal ?? null;
      if (typeof session.unsubscribe === "function") {
        session.unsubscribe();
        session.unsubscribe = null;
      }
    }
    session.updatedAt = new Date().toISOString();
    this.publishSessionUpdate(session, event.type);
  }

  publishSessionUpdate(session, eventType) {
    this.onEvent({
      type: eventType,
      session: serializeSession(session),
    });
  }
}

function resolveDetectedAgent(agentIdCandidate) {
  const agentId = assertKnownAgentId(agentIdCandidate);
  const agent = detectSupportedAgents().find((candidate) => candidate.id === agentId);
  if (!agent?.detected) {
    throw new Error(`${agent?.label ?? agentId} is not detected on PATH`);
  }
  return agent;
}

function createShellTerminalAgent(env) {
  const shell = resolveDefaultShell(env);
  if (!shell) {
    throw new Error("No executable default shell was found.");
  }
  return {
    id: "terminal_shell",
    label: "Terminal",
    detected: true,
    support: "supported",
    commands: [shell.command],
    command: shell.command,
    path: shell.path,
    launchArgs: ["-l"],
    confidence: "high",
    disabledReason: null,
  };
}

function resolveDefaultShell(env) {
  const candidates = [
    env.SHELL,
    process.platform === "win32" ? null : "/bin/zsh",
    process.platform === "win32" ? null : "/bin/bash",
    process.platform === "win32" ? "cmd.exe" : "/bin/sh",
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (!isExecutableFile(candidate)) {
      continue;
    }
    return {
      command: candidate.split(/[\\/]/).pop(),
      path: candidate,
    };
  }
  return null;
}

function isExecutableFile(candidate) {
  try {
    fs.accessSync(candidate, fs.constants.X_OK);
    return fs.statSync(candidate).isFile();
  } catch {
    return false;
  }
}

function rejectCommandPayload(args) {
  for (const field of FORBIDDEN_COMMAND_FIELDS) {
    if (Object.prototype.hasOwnProperty.call(args, field)) {
      throw new Error(`agent terminal rejected arbitrary command field: ${field}`);
    }
  }
}

function normalizeDimension(value, fallback) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.max(1, Math.min(500, Math.floor(parsed)));
}

function resolveHandoffState(backendSession) {
  if (backendSession.status === "unavailable") {
    return "external_confirmation_required";
  }
  const state = backendSession.handoffStatus ?? backendSession.handoff?.status;
  if (state === "attached" || state === "acknowledged") {
    return "writable";
  }
  return "blocked";
}

function resolveSessionStatus(backendSession) {
  if (backendSession.status === "unavailable") {
    return "fallback_required";
  }
  return resolveHandoffState(backendSession) === "writable"
    ? "running"
    : "handoff_required";
}

function serializeSession(session) {
  const { unsubscribe, ...serializable } = session;
  return serializable;
}

module.exports = {
  AgentTerminalSessionManager,
  createShellTerminalAgent,
  rejectCommandPayload,
  resolveDefaultShell,
  resolveHandoffState,
  serializeSession,
};
