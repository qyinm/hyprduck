function createAgentContextHandoff(options = {}) {
  const workspaceId = options.workspaceId ?? "default";
  const projectId = options.projectId ?? null;
  const nodeId = options.nodeId ?? null;
  const sourceId = options.sourceId ?? null;
  const sourceManifestPath = options.sourceManifestPath ?? null;
  const contextScope = options.contextScope ?? "workspace";

  return {
    mcp: {
      status: "available",
      toolHint: "Use HyprDuck MCP get_context_pack/read_context_pack for cited evidence.",
    },
    workspace: {
      workspaceId,
      projectId,
      nodeId,
      sourceId,
      sourceManifestPath,
    },
    context: {
      scope: contextScope,
      requiredBeforeFirstPrompt: true,
      attachInstructions: [
        `Workspace: ${workspaceId}`,
        "Ask the agent to call HyprDuck MCP get_context_pack before answering.",
        "Use cited evidence refs and page/source refs from the returned context pack.",
      ],
    },
    disclosure: {
      localPathsRedactedByDefault: true,
      externalAgentOwnsWorkflow: true,
    },
  };
}

function assertContextHandoffReady(handoff) {
  const missing = [];
  if (!handoff?.mcp?.status) missing.push("mcp.status");
  if (!handoff?.workspace?.workspaceId) missing.push("workspace.workspaceId");
  if (!handoff?.context?.scope) missing.push("context.scope");
  if (!Array.isArray(handoff?.context?.attachInstructions)) {
    missing.push("context.attachInstructions");
  }
  if (missing.length > 0) {
    throw new Error(`agent context handoff is incomplete: ${missing.join(", ")}`);
  }
  return handoff;
}

module.exports = {
  assertContextHandoffReady,
  createAgentContextHandoff,
};
