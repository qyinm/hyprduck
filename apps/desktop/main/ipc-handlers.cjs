const { dialog, ipcMain } = require("electron");
const { AgentTerminalSessionManager } = require("./agent-terminal-sessions.cjs");
const {
  DisabledAgentTerminalBackend,
  createDefaultAgentTerminalBackend,
} = require("./agent-terminal-backend.cjs");
const { createGhosttyNativeBackendFromEnv } = require("./agent-terminal-ghostty.cjs");

const AGENT_TERMINAL_EVENT = "hyprduck://agent-terminal";

async function registerIpcHandlers({
  getMainWindow,
  snapshot,
  runEngineCommand,
  maybeImportLegacySwiftConfig,
  brainReadScope,
  getModelsForProvider,
  applyWorkspaceCorrection,
  startAgentChat,
  stopAgentChat,
  startParse,
  retryFailedPages,
  cancelParse,
  openLocalArtifact,
  readSourceDetail,
  detectFormat,
}) {
  let agentTerminalSessions = null;

  function publishAgentTerminalEvent(payload) {
    const mainWindow = getMainWindow();
    if (!mainWindow || mainWindow.isDestroyed()) {
      return;
    }
    mainWindow.webContents.send(AGENT_TERMINAL_EVENT, payload);
  }

  const ghosttyProbe = await createGhosttyNativeBackendFromEnv();
  const backend = ghosttyProbe.enabled
    ? ghosttyProbe.backend ??
      new DisabledAgentTerminalBackend({
        reason: ghosttyProbe.reason ?? "Ghostty native backend is unavailable.",
      })
    : createDefaultAgentTerminalBackend();
  agentTerminalSessions = new AgentTerminalSessionManager({
    backend,
    getWorkspaceState: () => ({
      workspaceId: snapshot.lastWorkspaceId ?? "default",
      projectId: snapshot.lastProjectId ?? null,
      sourceId: snapshot.lastSourceId ?? null,
    }),
    onEvent: publishAgentTerminalEvent,
  });

  async function pickImportFile() {
    const result = await dialog.showOpenDialog(getMainWindow(), {
      properties: ["openFile"],
      filters: [{ name: "Documents", extensions: ["pdf", "docx", "doc"] }],
    });
    if (result.canceled || result.filePaths.length === 0) {
      return null;
    }
    const selectedPath = result.filePaths[0];
    const format = detectFormat(selectedPath);
    return format ? { path: selectedPath, format } : null;
  }

  ipcMain.handle("hyprduck:invoke", async (_event, command, args = {}) => {
    switch (command) {
      case "app_snapshot":
        return snapshot;
      case "pick_import_file":
        return pickImportFile();
      case "load_engine_config":
        await maybeImportLegacySwiftConfig();
        return runEngineCommand("load_config", { command: "load_config", payload: {} }).then(
          (response) => response.data,
        );
      case "save_engine_config":
        return runEngineCommand("save_config", {
          command: "save_config",
          payload: { config: args.payload },
        }).then((response) => response.data.config);
      case "validate_engine_config":
        await maybeImportLegacySwiftConfig();
        return runEngineCommand("validate_provider", {
          command: "validate_provider",
          payload: { config: args.payload ?? null },
        }).then((response) => response.data);
      case "engine_readiness":
        await maybeImportLegacySwiftConfig();
        return runEngineCommand("check_readiness", {
          command: "check_readiness",
          payload: {},
        }).then((response) => response.data);
      case "get_models_for_provider":
        return getModelsForProvider(args.providerSlug);
      case "load_workspace_project":
        return runEngineCommand("load_project", {
          command: "load_project",
          payload: {
            project_id: args.project_id ?? null,
            workspace_id: args.workspace_id ?? null,
          },
        }).then((response) => response.data);
      case "load_materialized_graph_snapshot": {
        const workspaceId = args.workspace_id ?? snapshot.lastWorkspaceId ?? "default";
        return runEngineCommand("read_graph_snapshot", {
          command: "read_graph_snapshot",
          payload: {
            scope: brainReadScope(workspaceId),
            includeLocalPaths: true,
          },
        }).then((response) => response.data);
      }
      case "apply_workspace_correction":
        return applyWorkspaceCorrection(args.correction);
      case "agent_chat_ask": {
        const request = args.request ?? {};
        const workspaceId =
          request.scope?.workspaceId ?? snapshot.lastWorkspaceId ?? "default";
        return runEngineCommand("agent_chat_ask", {
          command: "agent_chat_ask",
          payload: {
            ...request,
            scope: brainReadScope(workspaceId),
          },
        }).then((response) => response.data);
      }
      case "agent_chat_start":
        return startAgentChat(args.request ?? {});
      case "agent_chat_stop":
        return stopAgentChat(args.requestId);
      case "agent_terminal_list_agents":
        return agentTerminalSessions.listAgents(args);
      case "agent_terminal_create_session":
        return agentTerminalSessions.createSession(args);
      case "agent_terminal_snapshot_session":
        return agentTerminalSessions.snapshotSession(args);
      case "agent_terminal_write_session":
        return agentTerminalSessions.writeSession(args);
      case "agent_terminal_resize_session":
        return agentTerminalSessions.resizeSession(args);
      case "agent_terminal_kill_session":
        return agentTerminalSessions.killSession(args);
      case "start_parse":
        return startParse(args.request);
      case "retry_failed_pages":
        return retryFailedPages();
      case "cancel_parse":
        return cancelParse();
      case "open_saved_output":
        return openLocalArtifact(args.path, Boolean(args.reveal));
      case "open_local_artifact":
        return openLocalArtifact(args.path, Boolean(args.reveal));
      case "read_source_detail":
        return readSourceDetail(args);
      default:
        throw new Error(`unknown HyprDuck command: ${command}`);
    }
  });
}

module.exports = {
  AGENT_TERMINAL_EVENT,
  registerIpcHandlers,
};
