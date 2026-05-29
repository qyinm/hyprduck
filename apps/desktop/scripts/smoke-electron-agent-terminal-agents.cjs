#!/usr/bin/env node

const { app } = require("electron");
const {
  parseArgs,
  runAgentTerminalAgentSmoke,
} = require("./smoke-agent-terminal-agents.cjs");

app.whenReady().then(async () => {
  const args = parseArgs(process.argv.slice(2));
  const result = await runAgentTerminalAgentSmoke({
    agentIds: args.agents,
    timeoutMs: args.timeoutMs,
  });
  console.log(JSON.stringify(result, null, 2));
  app.exit(result.results.some((entry) => !entry.ok && !entry.skipped) ? 1 : 0);
}).catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : error);
  app.exit(1);
});
