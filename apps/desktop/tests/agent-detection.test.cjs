const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const assert = require("node:assert/strict");

const {
  assertKnownAgentId,
  detectSupportedAgents,
} = require("../main/agent-detection.cjs");

test("detectSupportedAgents detects supported agent commands from PATH", () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "hyprduck-agents-"));
  const codexPath = path.join(tempDir, "codex");
  fs.writeFileSync(codexPath, "#!/bin/sh\nexit 0\n");
  fs.chmodSync(codexPath, 0o755);

  const agents = detectSupportedAgents({ pathEnv: tempDir });
  const codex = agents.find((agent) => agent.id === "codex");
  const claude = agents.find((agent) => agent.id === "claude_code");

  assert.equal(codex.detected, true);
  assert.equal(codex.command, "codex");
  assert.equal(codex.path, codexPath);
  assert.equal(codex.confidence, "high");
  assert.equal(claude.detected, false);
  assert.equal(claude.command, null);
});

test("detectSupportedAgents does not expose generic shell as a v1 agent", () => {
  const agents = detectSupportedAgents({ pathEnv: process.env.PATH ?? "" });
  assert.equal(
    agents.some((agent) => agent.id === "shell" || agent.id === "generic_shell"),
    false,
  );
});

test("assertKnownAgentId rejects arbitrary command identifiers", () => {
  assert.equal(assertKnownAgentId("codex"), "codex");
  assert.throws(() => assertKnownAgentId("zsh"), /unknown supported agent id/);
  assert.throws(() => assertKnownAgentId("rm -rf"), /unknown supported agent id/);
});
