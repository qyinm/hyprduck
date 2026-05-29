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
    this.sessions = new Map();
  }

  listAgents() {
    return {
      agents: detectSupportedAgents(),
      shell: {
        available: false,
        reason: "Generic shell/custom commands are disabled in Agent Terminal v1.",
      },
    };
  }

  async createSession(args = {}) {
    rejectCommandPayload(args);
    const agentId = assertKnownAgentId(args.agentId ?? args.agent_id);
    const agent = detectSupportedAgents().find((candidate) => candidate.id === agentId);
    if (!agent?.detected) {
      throw new Error(`${agent?.label ?? agentId} is not detected on PATH`);
    }

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
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    this.sessions.set(sessionId, session);
    return session;
  }

  snapshotSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    return session;
  }

  async writeSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    const input = String(args.input ?? "");
    if (!input) {
      return { status: "ignored", reason: "empty input" };
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
    return this.backend.resize(session.backendSessionId, {
      cols: normalizeDimension(args.cols, 120),
      rows: normalizeDimension(args.rows, 36),
    });
  }

  async killSession(args = {}) {
    const session = this.requireSession(args.sessionId ?? args.session_id);
    const result = await this.backend.kill(session.backendSessionId);
    session.status = "closed";
    session.updatedAt = new Date().toISOString();
    return result;
  }

  requireSession(sessionId) {
    if (!sessionId || !this.sessions.has(sessionId)) {
      throw new Error(`unknown agent terminal session: ${sessionId ?? "(missing)"}`);
    }
    return this.sessions.get(sessionId);
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

module.exports = {
  AgentTerminalSessionManager,
  rejectCommandPayload,
  resolveHandoffState,
};
