const test = require("node:test");
const assert = require("node:assert/strict");

const {
  createGhosttyNativeBackendFromEnv,
  ghosttyNativeBackendEnabled,
} = require("../main/agent-terminal-ghostty.cjs");

test("ghosttyNativeBackendEnabled is opt-in", () => {
  assert.equal(ghosttyNativeBackendEnabled({}), false);
  assert.equal(
    ghosttyNativeBackendEnabled({
      HYPRDUCK_AGENT_TERMINAL_BACKEND: "ghostty-native",
    }),
    true,
  );
});

test("createGhosttyNativeBackendFromEnv reports missing spike module", async () => {
  const result = await createGhosttyNativeBackendFromEnv({
    HYPRDUCK_AGENT_TERMINAL_BACKEND: "ghostty-native",
  });
  assert.equal(result.enabled, true);
  assert.equal(result.backend, null);
  assert.match(result.reason, /HYPRDUCK_GHOSTTY_BACKEND_MODULE/);
});

test("createGhosttyNativeBackendFromEnv contains bad spike module failures", async () => {
  const result = await createGhosttyNativeBackendFromEnv({
    HYPRDUCK_AGENT_TERMINAL_BACKEND: "ghostty-native",
    HYPRDUCK_GHOSTTY_BACKEND_MODULE: "hyprduck-missing-ghostty-backend",
  });
  assert.equal(result.enabled, true);
  assert.equal(result.backend, null);
  assert.match(result.reason, /Ghostty native backend load failed/);
});
