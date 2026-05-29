#!/usr/bin/env node

const { AgentTerminalSessionManager } = require("../main/agent-terminal-sessions.cjs");
const { createPtyAgentTerminalBackend } = require("../main/agent-terminal-pty.cjs");

const DEFAULT_AGENT_IDS = ["codex", "claude_code"];
const DEFAULT_TIMEOUT_MS = 3500;
const AGENT_PATTERNS = {
  codex: /OpenAI Codex|Codex couldn't start|codex/i,
  claude_code: /Claude|CLAUDE\.md|claude/i,
  hermes: /hermes/i,
  pi_agent: /pi[- ]?agent|pi agent/i,
};

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const agentIds = args.agents ?? DEFAULT_AGENT_IDS;
  const timeoutMs = args.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const manager = new AgentTerminalSessionManager({
    backend: createPtyAgentTerminalBackend({ cwd: process.cwd() }),
    getWorkspaceState: () => ({
      workspaceId: "default",
      projectId: "workspace:default",
      sourceId: null,
    }),
  });
  const availableAgents = new Map(
    manager.listAgents().agents.map((agent) => [agent.id, agent]),
  );
  const results = [];

  for (const agentId of agentIds) {
    const agent = availableAgents.get(agentId);
    if (!agent?.detected) {
      results.push({
        agentId,
        ok: false,
        skipped: true,
        reason: `${agent?.label ?? agentId} is not detected on PATH`,
      });
      continue;
    }
    results.push(await smokeAgent(manager, agentId, timeoutMs));
  }

  console.log(JSON.stringify({ ok: results.every((result) => result.ok), results }, null, 2));
  if (results.some((result) => !result.ok && !result.skipped)) {
    process.exitCode = 1;
  }
}

async function smokeAgent(manager, agentId, timeoutMs) {
  let session = null;
  try {
    session = await manager.createSession({
      agentId,
      cols: 100,
      rows: 28,
    });
    await delay(timeoutMs);
    const snapshot = manager.snapshotSession({ sessionId: session.id });
    const output = snapshot.output ?? "";
    const stripped = stripAnsi(output);
    await manager.killSession({ sessionId: session.id });
    const pattern = AGENT_PATTERNS[agentId] ?? new RegExp(agentId, "i");
    const matched = pattern.test(stripped);
    return {
      agentId,
      ok: matched,
      backend: snapshot.backend.backend,
      handoffState: snapshot.handoffState,
      outputLength: output.length,
      preview: stripped.slice(0, 500),
      reason: matched ? null : `output did not match ${pattern}`,
    };
  } catch (error) {
    if (session?.id) {
      await manager.killSession({ sessionId: session.id }).catch(() => undefined);
    }
    return {
      agentId,
      ok: false,
      reason: error instanceof Error ? error.message : String(error),
    };
  }
}

function parseArgs(args) {
  const parsed = {};
  for (const arg of args) {
    if (arg.startsWith("--agents=")) {
      parsed.agents = arg
        .slice("--agents=".length)
        .split(",")
        .map((agent) => agent.trim())
        .filter(Boolean);
    }
    if (arg.startsWith("--timeout-ms=")) {
      const timeoutMs = Number(arg.slice("--timeout-ms=".length));
      if (Number.isFinite(timeoutMs) && timeoutMs > 0) {
        parsed.timeoutMs = timeoutMs;
      }
    }
  }
  return parsed;
}

function stripAnsi(value) {
  return String(value)
    .replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "");
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : error);
  process.exitCode = 1;
});
