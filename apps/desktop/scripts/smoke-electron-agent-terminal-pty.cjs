#!/usr/bin/env node

const { app } = require("electron");
const {
  createPtyAgentTerminalBackend,
} = require("../main/agent-terminal-pty.cjs");

app.whenReady().then(async () => {
  const backend = createPtyAgentTerminalBackend({ cwd: process.cwd() });
  const output = [];
  const session = await backend.createSession({
    sessionId: "electron-pty-smoke",
    agent: {
      label: "Shell",
      command: "sh",
      path: "/bin/sh",
      launchArgs: ["-lc", "printf hyprduck-electron-pty-smoke"],
    },
    handoff: {
      workspace: {
        workspaceId: "default",
        projectId: null,
        nodeId: null,
      },
    },
    cols: 80,
    rows: 24,
  });

  const timer = setTimeout(() => {
    console.error("electron PTY smoke timed out");
    app.exit(1);
  }, 3000);

  backend.subscribe(session.id, (event) => {
    if (event.data) {
      output.push(event.data);
    }
    if (event.type === "exit") {
      clearTimeout(timer);
      console.log(
        JSON.stringify({
          backend: session.backend,
          handoffStatus: session.handoffStatus,
          output: output.join("").trim(),
          exitCode: event.exitCode,
        }),
      );
      app.exit(event.exitCode ?? 0);
    }
  });
}).catch((error) => {
  console.error(error);
  app.exit(1);
});
