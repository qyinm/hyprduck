const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

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

test("createSession redacts local manifest paths from agent handoff", async () => {
  await withDetectedCodex(async () => {
    const manager = new AgentTerminalSessionManager({
      backend: createFakeBackend({ handoffStatus: "attached" }),
      getWorkspaceState: () => ({
        workspaceId: "workspace-1",
        sourceId: "source-1",
        sourceManifestPath: "/private/tmp/source-manifest.json",
      }),
    });

    const session = await manager.createSession({ agentId: "codex" });

    assert.equal(session.handoff.workspace.workspaceId, "workspace-1");
    assert.equal(session.handoff.workspace.sourceId, "source-1");
    assert.equal("sourceManifestPath" in session.handoff.workspace, false);
    assert.equal(session.handoff.disclosure.localPathsRedactedByDefault, true);
  });
});

test("backend lifecycle uses the backend session id", async () => {
  await withDetectedCodex(async () => {
    const backend = createFakeBackend({ backendSessionId: "native-session-1", handoffStatus: "attached" });
    const manager = new AgentTerminalSessionManager({ backend });

    const session = await manager.createSession({ agentId: "codex" });
    assert.equal(session.status, "running");
    const writeResult = await manager.writeSession({ sessionId: session.id, input: "hello" });
    await manager.resizeSession({ sessionId: session.id, cols: 100, rows: 30 });
    await manager.killSession({ sessionId: session.id });

    assert.equal(session.backendSessionId, "native-session-1");
    assert.equal(session.status, "closed");
    assert.equal(writeResult.status, "written");
    assert.deepEqual(backend.calls.write, ["native-session-1"]);
    assert.deepEqual(backend.calls.resize, ["native-session-1"]);
    assert.deepEqual(backend.calls.kill, ["native-session-1"]);
  });
});

test("writeSession blocks until context handoff is attached", async () => {
  await withDetectedCodex(async () => {
    const backend = createFakeBackend({ handoffStatus: "prepared" });
    const manager = new AgentTerminalSessionManager({ backend });

    const session = await manager.createSession({ agentId: "codex" });
    const result = await manager.writeSession({ sessionId: session.id, input: "hello" });

    assert.equal(session.status, "handoff_required");
    assert.equal(result.status, "blocked");
    assert.deepEqual(backend.calls.write, []);
  });
});

async function withDetectedCodex(callback) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "hyprduck-codex-"));
  const codexPath = path.join(tempDir, "codex");
  fs.writeFileSync(codexPath, "#!/bin/sh\nexit 0\n");
  fs.chmodSync(codexPath, 0o755);
  const previousPath = process.env.PATH;
  process.env.PATH = tempDir;
  try {
    await callback();
  } finally {
    process.env.PATH = previousPath;
  }
}

function createFakeBackend(options = {}) {
  const calls = {
    create: [],
    write: [],
    resize: [],
    kill: [],
  };
  return {
    calls,
    async createSession(args) {
      calls.create.push(args);
      return {
        id: options.backendSessionId ?? args.sessionId,
        backend: "fake",
        status: "running",
        handoffStatus: options.handoffStatus ?? "attached",
      };
    },
    async write(sessionId) {
      calls.write.push(sessionId);
      return { status: "written" };
    },
    async resize(sessionId) {
      calls.resize.push(sessionId);
      return { status: "resized" };
    },
    async kill(sessionId) {
      calls.kill.push(sessionId);
      return { status: "closed" };
    },
    subscribe() {
      return () => {};
    },
    snapshotStatus() {
      return { backend: "fake", available: true };
    },
  };
}
