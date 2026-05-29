function createExternalGhosttyFallback(agent, handoff) {
  return {
    type: "external_ghostty",
    label: "External Ghostty",
    available: true,
    agentId: agent.id,
    agentCommand: agent.command,
    attachInstructions: [
      `Open Ghostty and run: ${agent.command}`,
      ...handoff.context.attachInstructions,
    ],
    handoff,
  };
}

function assertExternalFallbackReady(fallback) {
  const missing = [];
  if (fallback?.type !== "external_ghostty") missing.push("type");
  if (!fallback?.agentId) missing.push("agentId");
  if (!fallback?.agentCommand) missing.push("agentCommand");
  if (!Array.isArray(fallback?.attachInstructions)) {
    missing.push("attachInstructions");
  }
  if (!fallback?.handoff?.mcp?.status) missing.push("handoff.mcp.status");
  if (missing.length > 0) {
    throw new Error(`external Ghostty fallback is incomplete: ${missing.join(", ")}`);
  }
  return fallback;
}

module.exports = {
  assertExternalFallbackReady,
  createExternalGhosttyFallback,
};
