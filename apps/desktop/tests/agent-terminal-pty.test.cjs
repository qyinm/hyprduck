const test = require("node:test");
const assert = require("node:assert/strict");

const {
  createPtyAgentTerminalBackend,
  isPackagedAppResource,
} = require("../main/agent-terminal-pty.cjs");
const {
  createDefaultAgentTerminalBackend,
} = require("../main/agent-terminal-backend.cjs");

test("pty backend spawns the detected agent command and forwards lifecycle events", async () => {
  const fakePty = createFakePty();
  const backend = createPtyAgentTerminalBackend({
    pty: fakePty,
    cwd: "/tmp",
    env: { PATH: "/bin", HOME: "/tmp" },
  });
  const session = await backend.createSession({
    sessionId: "session-1",
    agent: {
      label: "Codex",
      command: "codex",
      path: "/usr/local/bin/codex",
      launchArgs: [],
    },
    handoff: {
      workspace: {
        workspaceId: "workspace-1",
        projectId: "project-1",
        nodeId: "node-1",
      },
    },
    cols: 100,
    rows: 28,
  });
  const events = [];
  backend.subscribe("session-1", (event) => events.push(event));

  fakePty.process.emitData("ready");
  await backend.write("session-1", "hello");
  await backend.resize("session-1", { cols: 120, rows: 32 });
  fakePty.process.emitExit({ exitCode: 0, signal: 0 });

  assert.equal(session.backend, "node_pty");
  assert.equal(session.handoffStatus, "attached");
  assert.equal(fakePty.spawnArgs.command, "/usr/local/bin/codex");
  assert.deepEqual(fakePty.spawnArgs.args, []);
  assert.equal(fakePty.spawnArgs.options.cols, 100);
  assert.equal(fakePty.spawnArgs.options.rows, 28);
  assert.equal(fakePty.spawnArgs.options.env.ETYMA_WORKSPACE_ID, "workspace-1");
  assert.deepEqual(fakePty.process.writes, ["hello"]);
  assert.deepEqual(fakePty.process.resizes, [{ cols: 120, rows: 32 }]);
  assert.equal(events[0].data, "ready");
  assert.equal(events[1].type, "exit");
  assert.equal(events[1].exitCode, 0);
  assert.deepEqual(await backend.write("session-1", "late"), {
    status: "blocked",
    reason: "terminal session is closed",
  });
});

test("pty backend replays fast startup output to late subscribers", async () => {
  const fakePty = createFakePty();
  const backend = createPtyAgentTerminalBackend({
    pty: fakePty,
    cwd: "/tmp",
    env: { PATH: "/bin", HOME: "/tmp" },
  });
  await backend.createSession({
    sessionId: "session-1",
    agent: {
      label: "Codex",
      command: "codex",
      path: "/usr/local/bin/codex",
      launchArgs: [],
    },
    handoff: {
      workspace: {
        workspaceId: "workspace-1",
        projectId: null,
        nodeId: null,
      },
    },
    cols: 80,
    rows: 24,
  });

  fakePty.process.emitData("boot");
  const events = [];
  backend.subscribe("session-1", (event) => events.push(event));

  assert.equal(events.length, 1);
  assert.equal(events[0].data, "boot");
});

test("default agent terminal backend uses PTY when it is available", () => {
  const backend = createDefaultAgentTerminalBackend({
    pty: createFakePty(),
    cwd: "/tmp",
    env: { PATH: "/bin", HOME: "/tmp" },
  });

  assert.equal(backend.snapshotStatus().backend, "node_pty");
});

test("packaged app resources are not mutated at runtime", () => {
  const originalResourcesPath = process.resourcesPath;
  process.resourcesPath = "/Applications/Etyma.app/Contents/Resources";
  try {
    assert.equal(
      isPackagedAppResource(
        "/Applications/Etyma.app/Contents/Resources/app.asar.unpacked/node_modules/node-pty/prebuilds/darwin-arm64/spawn-helper",
      ),
      true,
    );
    assert.equal(
      isPackagedAppResource(
        "/Users/hippoo/dev/Etyma/apps/desktop/node_modules/node-pty/prebuilds/darwin-arm64/spawn-helper",
      ),
      false,
    );
  } finally {
    process.resourcesPath = originalResourcesPath;
  }
});

function createFakePty() {
  const process = {
    pid: 123,
    writes: [],
    resizes: [],
    dataHandler: null,
    exitHandler: null,
    write(input) {
      this.writes.push(input);
    },
    resize(cols, rows) {
      this.resizes.push({ cols, rows });
    },
    kill() {},
    onData(handler) {
      this.dataHandler = handler;
    },
    onExit(handler) {
      this.exitHandler = handler;
    },
    emitData(data) {
      this.dataHandler(data);
    },
    emitExit(exit) {
      this.exitHandler(exit);
    },
  };
  return {
    process,
    spawnArgs: null,
    spawn(command, args, options) {
      this.spawnArgs = { command, args, options };
      return process;
    },
  };
}
