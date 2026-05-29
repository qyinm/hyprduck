const test = require("node:test");
const assert = require("node:assert/strict");

const { createAgentContextHandoff } = require("../main/agent-terminal-context.cjs");
const {
  assertExternalFallbackReady,
  createExternalGhosttyFallback,
} = require("../main/agent-terminal-fallbacks.cjs");

test("external Ghostty fallback includes agent and context attach instructions", () => {
  const fallback = createExternalGhosttyFallback(
    { id: "codex", command: "codex" },
    createAgentContextHandoff({ workspaceId: "default" }),
  );

  assert.equal(assertExternalFallbackReady(fallback), fallback);
  assert.equal(fallback.type, "external_ghostty");
  assert.equal(fallback.agentCommand, "codex");
  assert.ok(
    fallback.attachInstructions.some((instruction) =>
      instruction.includes("get_context_pack"),
    ),
  );
});
