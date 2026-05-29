const test = require("node:test");
const assert = require("node:assert/strict");

const { AgentTerminalSessionManager } = require("../main/agent-terminal-sessions.cjs");

test("listAgents keeps generic shell disabled", () => {
  const manager = new AgentTerminalSessionManager();
  const result = manager.listAgents();
  assert.equal(result.shell.available, false);
  assert.equal(result.agents.some((agent) => agent.id === "generic_shell"), false);
});

test("createSession rejects arbitrary command fields", async () => {
  const manager = new AgentTerminalSessionManager();
  await assert.rejects(
    () => manager.createSession({ agentId: "codex", command: "zsh" }),
    /arbitrary command field/,
  );
});

test("createSession rejects unknown agent ids", async () => {
  const manager = new AgentTerminalSessionManager();
  await assert.rejects(() => manager.createSession({ agentId: "zsh" }), /unknown/);
});
