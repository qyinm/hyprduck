const { app, BrowserWindow, dialog, ipcMain, net, protocol, shell } = require("electron");
const crypto = require("node:crypto");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { ensureHyprduckShellCommand, hostTriple } = require("./main/cli-shim.cjs");
const {
  EngineRuntime,
  runOneShotEngineCommand: runOneShotEngineRequest,
} = require("./main/engine-runtime.cjs");
const {
  maybeImportLegacySwiftConfig: importLegacySwiftConfig,
} = require("./main/legacy-config.cjs");
const { AgentTerminalSessionManager } = require("./main/agent-terminal-sessions.cjs");
const {
  DisabledAgentTerminalBackend,
  createDefaultAgentTerminalBackend,
} = require("./main/agent-terminal-backend.cjs");
const { createGhosttyNativeBackendFromEnv } = require("./main/agent-terminal-ghostty.cjs");

const SNAPSHOT_EVENT = "hyprduck://snapshot";
const AGENT_TERMINAL_EVENT = "hyprduck://agent-terminal";
const AGENT_CHAT_EVENT = "hyprduck://agent-chat";
const SOURCE_PREVIEW_PROTOCOL = "hyprduck-source";
const MAX_PROGRESS_LOG = 80;
const MAX_INLINE_TEXT_PREVIEW_BYTES = 2 * 1024 * 1024;

protocol.registerSchemesAsPrivileged([
  {
    scheme: SOURCE_PREVIEW_PROTOCOL,
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      corsEnabled: true,
      stream: true,
    },
  },
]);

const snapshot = {
  activeJob: null,
  progressLog: [],
  lastResult: null,
  lastProjectId: null,
  lastWorkspaceId: null,
  lastSourceId: null,
  lastSourceManifestPath: null,
  workspaceRevision: 0,
};

let mainWindow = null;
let engineRuntime = null;
let engineRuntimeBinarySignature = null;
let providerModelCatalogPromise = null;
let graphRebuildQueue = Promise.resolve();
let agentTerminalSessions = null;
const activeAgentChatStreams = new Map();
const sourcePreviewPaths = new Map();
let autoUpdateStarted = false;
let sourcePreviewProtocolRegistered = false;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1160,
    height: 860,
    minWidth: 900,
    minHeight: 600,
    title: "HyprDuck",
    titleBarStyle: "hiddenInset",
    trafficLightPosition: { x: 18, y: 18 },
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  if (process.env.VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(process.env.VITE_DEV_SERVER_URL);
  } else {
    mainWindow.loadFile(path.join(__dirname, "dist", "index.html"));
  }
  if (process.env.HYPRDUCK_DEBUG_RENDERER_LOGS === "1") {
    mainWindow.webContents.on("did-fail-load", (_event, code, description, validatedURL) => {
      console.error("renderer failed to load:", { code, description, validatedURL });
    });
    mainWindow.webContents.on("console-message", (_event, level, message, line, sourceId) => {
      console.error("renderer console:", { level, message, line, sourceId });
    });
    mainWindow.webContents.on("render-process-gone", (_event, details) => {
      console.error("renderer process gone:", details);
    });
  }
}

app.whenReady().then(async () => {
  registerSourcePreviewProtocol();
  await registerIpcHandlers();
  try {
    await maybeImportLegacySwiftConfig();
  } catch (error) {
    console.error("legacy config migration skipped:", error);
  }
  try {
    ensureHyprduckShellCommand(app);
  } catch (error) {
    console.error("hyprduck shell command setup skipped:", error);
  }
  createWindow();
  startAutoUpdateChecks();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    createWindow();
  }
});

app.on("will-quit", () => {
  resetEngineRuntime();
});

async function registerIpcHandlers() {
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
      case "brain_health": {
        const workspaceId = args.workspace_id ?? snapshot.lastWorkspaceId ?? "default";
        return runEngineCommand("get_brain_health", {
          command: "get_brain_health",
          payload: {
            scope: brainReadScope(workspaceId),
          },
        }).then((response) => response.data);
      }
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
      case "answer_workspace_project":
        return runEngineCommand("answer_project", {
          command: "answer_project",
          payload: args.request,
        }).then((response) => response.data.answer);
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

function registerSourcePreviewProtocol() {
  if (sourcePreviewProtocolRegistered) {
    return;
  }
  protocol.handle(SOURCE_PREVIEW_PROTOCOL, async (request) => {
    const url = new URL(request.url);
    const token = url.hostname;
    const entry = sourcePreviewPaths.get(token);
    if (!entry || !fs.existsSync(entry.path)) {
      return new Response("Source preview not found.", { status: 404 });
    }
    return net.fetch(pathToFileURL(entry.path).toString());
  });
  sourcePreviewProtocolRegistered = true;
}

function publishAgentTerminalEvent(payload) {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return;
  }
  mainWindow.webContents.send(AGENT_TERMINAL_EVENT, payload);
}

function publishAgentChatEvent(payload) {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return;
  }
  mainWindow.webContents.send(AGENT_CHAT_EVENT, payload);
}

function startAgentChat(request) {
  const requestId = `agent_${crypto.randomUUID()}`;
  const conversationId = request.conversationId || `chat_${crypto.randomUUID()}`;
  const assistantMessageId =
    request.assistantMessageId || `msg_${crypto.randomUUID().replaceAll("-", "")}`;
  const workspaceId = request.scope?.workspaceId ?? snapshot.lastWorkspaceId ?? "default";
  const payload = {
    ...request,
    conversationId,
    assistantMessageId,
    scope: brainReadScope(workspaceId),
  };
  const streamState = { requestId, stopped: false };
  activeAgentChatStreams.set(requestId, streamState);

  setImmediate(() => {
    if (!activeAgentChatStreams.has(requestId)) {
      return;
    }
    void runEngineCommand(
      "agent_chat_ask",
      {
        command: "agent_chat_ask",
        payload,
      },
      {
        onEvent: (event) => {
          if (!activeAgentChatStreams.has(requestId)) {
            return;
          }
          if (!event || typeof event !== "object") {
            return;
          }
          publishAgentChatEvent({ requestId, ...event });
        },
      },
    )
      .catch((error) => {
        const active = activeAgentChatStreams.get(requestId);
        if (!active || active.stopped) {
          return;
        }
        publishAgentChatEvent({
          requestId,
          type: "error",
          code: error.code ?? "runtime_error",
          message: error.message,
        });
      })
      .finally(() => {
        activeAgentChatStreams.delete(requestId);
      });
  });

  return { requestId, conversationId, assistantMessageId };
}

function stopAgentChat(requestId) {
  if (!requestId || !activeAgentChatStreams.has(requestId)) {
    return { stopped: false };
  }
  const active = activeAgentChatStreams.get(requestId);
  active.stopped = true;
  resetEngineRuntime();
  publishAgentChatEvent({
    requestId,
    type: "stopped",
    partialText: "",
  });
  activeAgentChatStreams.delete(requestId);
  return { stopped: true };
}

function startAutoUpdateChecks() {
  if (autoUpdateStarted || !app.isPackaged) {
    return;
  }
  autoUpdateStarted = true;
  const { updateElectronApp } = require("update-electron-app");
  updateElectronApp({
    repo: "qyinm/hyprduck",
    updateInterval: "10 minutes",
    logger: {
      log: console.error,
      debug: console.error,
      info: console.error,
      warn: console.error,
      error: console.error,
    },
  });
}

function brainReadScope(workspaceId) {
  return {
    workspaceId,
    rootDir: ensureHyprduckApplicationSupportPath(),
  };
}

async function pickImportFile() {
  const result = await dialog.showOpenDialog(mainWindow, {
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

async function applyWorkspaceCorrection(correction) {
  const response = await runEngineCommand("apply_correction", {
    command: "apply_correction",
    payload: correction,
  });
  const project = response.data.project;
  snapshot.lastProjectId = project.summary.projectId;
  pushProgressEntry("correction_applied", `Applied correction in ${project.summary.title}`);
  publishSnapshot();
  return project;
}

async function startParse(request) {
  if (snapshot.activeJob) {
    throw new Error("an import is already running");
  }

  const outputName = path.basename(request.path, path.extname(request.path)) || "document";
  const storageRoot = ensureHyprduckApplicationSupportPath();
  const parseRequest = {
    version: "1",
    template: "General",
    input: {
      path: request.path,
      format: formatForEngine(request.format),
    },
    options: {
      preserve_images: true,
      emit_structured_json: false,
      emit_svg: false,
      language_hints: [],
      debug_request_path: null,
      debug_result_path: null,
    },
    output: {
      root_dir: storageRoot,
      name: outputName,
      workspace_id: "default",
      source_id: null,
    },
  };

  snapshot.activeJob = {
    jobId: nextJobId(),
    filePath: request.path,
    format: request.format,
    status: "imported",
    progressPercent: 4,
    lastMessage: "Queued parse request",
  };
  snapshot.progressLog = [];
  publishSnapshot();

  try {
    const response = await runEngineCommand(
      "parse",
      { command: "parse", payload: parseRequest },
      { onEvent: applyRuntimeProgressLine },
    );
    const data = response?.data;
    const result = data?.result;
    if (!result) {
      markFailed("engine returned success response but missing result payload");
      return;
    }

    snapshot.lastResult = {
      savedOutputPath: data.saved_output_path ?? null,
      successCount: result.success_count ?? 0,
      failedCount: result.failed_count ?? 0,
      markdown: result.markdown,
    };
    snapshot.lastProjectId = null;
    snapshot.lastWorkspaceId = data.source_manifest?.workspace_id ?? null;
    snapshot.lastSourceId = data.source_manifest?.source_id ?? null;
    snapshot.lastSourceManifestPath = data.source_manifest?.manifest_path ?? null;
    pushProgressEntry(
      "completed",
      data.saved_output_path ?? "Parse completed without a saved output path",
    );

    if (data.saved_output_path) {
      try {
        if (snapshot.activeJob) {
          snapshot.activeJob.status = "packaging";
          snapshot.activeJob.progressPercent = 100;
          snapshot.activeJob.lastMessage = "Packaging citation evidence";
          pushProgressEntry("packaging", "Packaging citation evidence");
          publishSnapshot();
        }
        const sourceManifest = data.source_manifest ?? null;
        const project = await compileWorkspaceProject(
          data.saved_output_path,
          request.path,
          sourceManifest,
          { skipGraphGeneration: true },
        );
        snapshot.lastProjectId = project.projectId;
        snapshot.lastWorkspaceId = project.workspaceId ?? snapshot.lastWorkspaceId;
        snapshot.lastSourceId = project.sourceId ?? snapshot.lastSourceId;
        snapshot.workspaceRevision += 1;
        pushProgressEntry("compile", `Compiled knowledge workspace ${project.projectId}`);
        if (snapshot.activeJob) {
          snapshot.activeJob.status = "citation_ready";
          snapshot.activeJob.progressPercent = 94;
          snapshot.activeJob.lastMessage = "Citation-ready evidence is available";
          pushProgressEntry("citation_ready", "Citation-ready evidence is available");
        }
        let graphRebuildQueued = false;
        if (sourceManifest) {
          graphRebuildQueued = true;
          if (snapshot.activeJob) {
            snapshot.activeJob.status = "citation_ready";
            snapshot.activeJob.progressPercent = 96;
            snapshot.activeJob.lastMessage = "Preparing context graph";
          }
          pushProgressEntry("context", "Preparing context graph");
          enqueueWorkspaceGraphRebuild(
            data.saved_output_path,
            request.path,
            sourceManifest,
            snapshot.activeJob?.jobId ?? null,
          );
        }
        const graphGenerationMessage = graphGenerationNonBlockingMessage(project);
        if (graphGenerationMessage) {
          pushProgressEntry("compile", graphGenerationMessage);
        }
        if (graphRebuildQueued) {
          publishSnapshot();
          return;
        }
      } catch (error) {
        snapshot.lastProjectId = null;
        markFailed(`Knowledge workspace compile failed: ${error.message}`);
        return;
      }
    }

    snapshot.activeJob = null;
    publishSnapshot();
  } catch (error) {
    if (!snapshot.activeJob) {
      return;
    }
    markFailed(error.message);
  }
}

async function retryFailedPages() {
  if (snapshot.activeJob) {
    throw new Error("an import is already running");
  }
  if (!snapshot.lastSourceManifestPath) {
    throw new Error("No source manifest is available for failed-page retry.");
  }

  const manifestPath = resolveKnownWorkspacePath(snapshot.lastSourceManifestPath);
  const sourceManifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const failedPages = (sourceManifest.pages ?? []).filter((page) => page.error_message);
  if (failedPages.length === 0) {
    throw new Error("No failed pages are available to retry.");
  }

  const sourcePath = sourceManifest.original_path || sourceManifest.source_path;
  snapshot.activeJob = {
    jobId: nextJobId(),
    filePath: sourcePath,
    format: sourceManifest.format,
    status: "imported",
    progressPercent: 4,
    lastMessage: `Queued retry for ${failedPages.length} failed page${
      failedPages.length === 1 ? "" : "s"
    }`,
  };
  snapshot.progressLog = [];
  pushProgressEntry("retry", snapshot.activeJob.lastMessage);
  publishSnapshot();

  try {
    const parseResponse = await runEngineCommand(
      "parse",
      {
        command: "parse",
        payload: {
          version: "1",
          template: "General",
          input: {
            path: sourcePath,
            format: formatForEngine(sourceManifest.format),
          },
          options: {
            preserve_images: true,
            emit_structured_json: false,
            emit_svg: false,
            language_hints: [],
            debug_request_path: null,
            debug_result_path: null,
          },
          output: null,
        },
      },
      { onEvent: applyRuntimeProgressLine },
    );
    const parsedPages = parseResponse.data?.result?.pages ?? [];
    const retryPages = failedPages.map((failedPage) => {
      const parsedPage = parsedPages.find((page) => page.index === failedPage.index);
      const markdown = parsedPage?.markdown ?? null;
      const plainText = parsedPage?.plain_text ?? null;
      const retryError =
        parsedPage?.error_message ??
        (!markdown && !plainText ? "retry produced no page artifact" : null);
      return {
        pageIndex: failedPage.index,
        markdown,
        plainText,
        imageAssetPath: null,
        errorMessage: retryError,
      };
    });

    if (snapshot.activeJob) {
      snapshot.activeJob.progressPercent = 94;
      snapshot.activeJob.lastMessage = "Updating failed page artifacts";
      pushProgressEntry("retry", "Updating failed page artifacts");
      publishSnapshot();
    }

    const retryResponse = await runEngineCommand("retry_failed_pages", {
      command: "retry_failed_pages",
      payload: {
        sourceManifestPath: manifestPath,
        pages: retryPages,
      },
    });
    const retryData = retryResponse.data;
    const updatedManifest = retryData.sourceManifest;
    const remainingFailedCount = retryData.remainingFailedCount ?? 0;
    const markdown = fs.existsSync(updatedManifest.markdown_path)
      ? fs.readFileSync(updatedManifest.markdown_path, "utf8")
      : "";

    snapshot.lastResult = {
      savedOutputPath: updatedManifest.markdown_path ?? null,
      successCount: (updatedManifest.pages ?? []).length - remainingFailedCount,
      failedCount: remainingFailedCount,
      markdown,
    };
    snapshot.lastProjectId = null;
    snapshot.lastWorkspaceId = updatedManifest.workspace_id ?? snapshot.lastWorkspaceId;
    snapshot.lastSourceId = updatedManifest.source_id ?? snapshot.lastSourceId;
    snapshot.lastSourceManifestPath = updatedManifest.manifest_path ?? manifestPath;
    pushProgressEntry(
      "retry",
      `Retried ${retryData.retriedPageCount ?? 0} failed page${
        retryData.retriedPageCount === 1 ? "" : "s"
      }; ${remainingFailedCount} still failed`,
    );

    if (snapshot.activeJob) {
      snapshot.activeJob.status = "packaging";
      snapshot.activeJob.progressPercent = 100;
      snapshot.activeJob.lastMessage = "Packaging citation evidence after retry";
      pushProgressEntry("packaging", "Packaging citation evidence after retry");
      publishSnapshot();
    }
    const project = await compileWorkspaceProject(
      updatedManifest.markdown_path,
      updatedManifest.source_path,
      updatedManifest,
      { skipGraphGeneration: true },
    );
    snapshot.lastProjectId = project.projectId;
    snapshot.lastWorkspaceId = project.workspaceId ?? snapshot.lastWorkspaceId;
    snapshot.lastSourceId = project.sourceId ?? snapshot.lastSourceId;
    snapshot.workspaceRevision += 1;
    pushProgressEntry("compile", `Compiled knowledge workspace ${project.projectId}`);
    if (snapshot.activeJob) {
      snapshot.activeJob.status = "citation_ready";
      snapshot.activeJob.progressPercent = 94;
      snapshot.activeJob.lastMessage = "Citation-ready evidence is available";
      pushProgressEntry("citation_ready", "Citation-ready evidence is available after retry");
    }

    if (snapshot.activeJob) {
      snapshot.activeJob.status = "citation_ready";
      snapshot.activeJob.progressPercent = 96;
      snapshot.activeJob.lastMessage = "Preparing context graph";
    }
    pushProgressEntry("context", "Preparing context graph");
    enqueueWorkspaceGraphRebuild(
      updatedManifest.markdown_path,
      updatedManifest.source_path,
      updatedManifest,
      snapshot.activeJob?.jobId ?? null,
    );
    publishSnapshot();
  } catch (error) {
    if (!snapshot.activeJob) {
      return;
    }
    markFailed(error.message);
  }
}

function applyRuntimeProgressLine(line) {
  try {
    const event = typeof line === "string" ? JSON.parse(line) : line;
    applyProgressEvent(event);
  } catch {
    // Non-event stderr is ignored; engine failures still arrive on stdout.
  }
}

function hyprduckApplicationSupportPath() {
  return path.join(app.getPath("appData"), "HyprDuck");
}

function ensureHyprduckApplicationSupportPath() {
  const storageRoot = hyprduckApplicationSupportPath();
  fs.mkdirSync(storageRoot, { recursive: true });
  return storageRoot;
}

function engineEnvironment() {
  return {
    ...process.env,
    HYPRDUCK_OUTPUT_DIR: ensureHyprduckApplicationSupportPath(),
  };
}

async function cancelParse() {
  if (!snapshot.activeJob) {
    return;
  }
  resetEngineRuntime();
  markFailed("Parse canceled");
}

async function openSavedOutput(outputPath, reveal) {
  if (reveal) {
    shell.showItemInFolder(outputPath);
    return;
  }
  const error = await shell.openPath(outputPath);
  if (error) {
    throw new Error(error);
  }
}

async function openLocalArtifact(outputPath, reveal) {
  const safePath = resolveKnownWorkspacePath(outputPath);
  if (reveal) {
    if (!fs.existsSync(safePath)) {
      throw new Error(`Cannot reveal missing local artifact: ${safePath}`);
    }
    if (fs.statSync(safePath).isDirectory()) {
      const error = await shell.openPath(safePath);
      if (error) {
        throw new Error(error);
      }
      return;
    }
    shell.showItemInFolder(safePath);
    return;
  }
  const error = await shell.openPath(safePath);
  if (error) {
    throw new Error(error);
  }
}

function readSourceDetail(args = {}) {
  const sourceId = String(args.sourceId ?? "");
  const originalPath = String(args.originalPath ?? "");
  const sourcePath = String(args.sourcePath ?? "");
  const markdownPath = String(args.markdownPath ?? "");
  const format = String(args.format ?? "").toLowerCase();
  const originalCandidate = originalPath || sourcePath;
  const original = readOriginalPreview([originalPath, sourcePath], format);
  const markdown = readMarkdownPreview(markdownPath);
  return {
    sourceId,
    fileName: path.basename(originalCandidate || markdownPath || sourceId || "Source"),
    format,
    originalPath,
    sourcePath,
    markdownPath,
    original,
    markdown,
  };
}

function readOriginalPreview(candidatePaths, format) {
  const resolved = resolveFirstKnownWorkspacePath(candidatePaths);
  if (!resolved.path) {
    return {
      kind: "missing",
      previewUrl: null,
      text: null,
      truncated: false,
      error: resolved.error ?? "Original file is not available.",
    };
  }
  if (!fs.existsSync(resolved.path)) {
    return {
      kind: "missing",
      previewUrl: null,
      text: null,
      truncated: false,
      error: "Original file is missing.",
    };
  }
  if (isPdfPreview(format, resolved.path)) {
    return {
      kind: "pdf",
      previewUrl: createSourcePreviewUrl(resolved.path),
      text: null,
      truncated: false,
      error: null,
    };
  }
  if (isTextPreview(format, resolved.path)) {
    const stat = fs.statSync(resolved.path);
    const byteLimit = Math.min(stat.size, MAX_INLINE_TEXT_PREVIEW_BYTES);
    const buffer = Buffer.alloc(byteLimit);
    const fd = fs.openSync(resolved.path, "r");
    try {
      fs.readSync(fd, buffer, 0, byteLimit, 0);
    } finally {
      fs.closeSync(fd);
    }
    return {
      kind: "text",
      previewUrl: null,
      text: buffer.toString("utf8"),
      truncated: stat.size > MAX_INLINE_TEXT_PREVIEW_BYTES,
      error: null,
    };
  }
  return {
    kind: "unsupported",
    previewUrl: null,
    text: null,
    truncated: false,
    error: "Inline preview is not available for this file type.",
  };
}

function resolveFirstKnownWorkspacePath(candidatePaths) {
  const errors = [];
  for (const candidatePath of candidatePaths) {
    if (!candidatePath) {
      continue;
    }
    const resolved = tryResolveKnownWorkspacePath(candidatePath);
    if (resolved.path && fs.existsSync(resolved.path)) {
      return resolved;
    }
    if (resolved.error) {
      errors.push(resolved.error);
    }
  }
  return {
    path: null,
    error: errors[0] ?? "No workspace-backed source file is available.",
  };
}

function readMarkdownPreview(markdownPath) {
  const resolved = tryResolveKnownWorkspacePath(markdownPath);
  if (!resolved.path) {
    return {
      text: null,
      missing: true,
      error: resolved.error ?? "Parsed markdown is not available.",
    };
  }
  if (!fs.existsSync(resolved.path)) {
    return {
      text: null,
      missing: true,
      error: "Parsed markdown file is missing.",
    };
  }
  try {
    return {
      text: fs.readFileSync(resolved.path, "utf8"),
      missing: false,
      error: null,
    };
  } catch (error) {
    return {
      text: null,
      missing: true,
      error: error.message,
    };
  }
}

function tryResolveKnownWorkspacePath(candidatePath) {
  try {
    return { path: resolveKnownWorkspacePath(candidatePath), error: null };
  } catch (error) {
    return { path: null, error: error.message };
  }
}

function createSourcePreviewUrl(safePath) {
  pruneSourcePreviewPaths();
  const token = crypto.randomUUID();
  sourcePreviewPaths.set(token, { path: safePath, createdAt: Date.now() });
  return `${SOURCE_PREVIEW_PROTOCOL}://${token}/${encodeURIComponent(path.basename(safePath))}`;
}

function pruneSourcePreviewPaths() {
  const cutoff = Date.now() - 60 * 60 * 1000;
  for (const [token, entry] of sourcePreviewPaths.entries()) {
    if (entry.createdAt < cutoff) {
      sourcePreviewPaths.delete(token);
    }
  }
  while (sourcePreviewPaths.size > 200) {
    const firstToken = sourcePreviewPaths.keys().next().value;
    if (!firstToken) {
      return;
    }
    sourcePreviewPaths.delete(firstToken);
  }
}

function isPdfPreview(format, filePath) {
  return format === "pdf" || path.extname(filePath).toLowerCase() === ".pdf";
}

function isTextPreview(format, filePath) {
  const extension = path.extname(filePath).toLowerCase();
  return (
    ["txt", "md", "markdown", "csv", "json", "yaml", "yml"].includes(format) ||
    [".txt", ".md", ".markdown", ".csv", ".json", ".yaml", ".yml"].includes(extension)
  );
}

function resolveKnownWorkspacePath(candidatePath) {
  if (!candidatePath || typeof candidatePath !== "string") {
    throw new Error("Missing local artifact path.");
  }
  const storageRoot = path.resolve(ensureHyprduckApplicationSupportPath());
  const expandedPath = candidatePath.startsWith("~/")
    ? path.join(app.getPath("home"), candidatePath.slice(2))
    : candidatePath;
  const candidates = path.isAbsolute(expandedPath)
    ? [expandedPath]
    : [
        path.join(storageRoot, expandedPath),
        path.join(storageRoot, "default", expandedPath),
      ];
  const resolvedPath =
    candidates.map((candidate) => path.resolve(candidate)).find((candidate) => {
      const relativePath = path.relative(storageRoot, candidate);
      return (
        relativePath.length > 0 &&
        !relativePath.startsWith("..") &&
        !path.isAbsolute(relativePath) &&
        fs.existsSync(candidate)
      );
    }) ?? path.resolve(candidates[0]);
  const relativePath = path.relative(storageRoot, resolvedPath);
  if (
    relativePath.startsWith("..") ||
    path.isAbsolute(relativePath) ||
    relativePath.length === 0
  ) {
    throw new Error("Refusing to open a path outside the HyprDuck workspace.");
  }
  return resolvedPath;
}

async function compileWorkspaceProject(
  sourceMarkdownPath,
  sourceDocumentPath,
  sourceManifest,
  options = {},
) {
  const request = {
    command: "compile_project",
    payload: {
      source_markdown_path: sourceMarkdownPath,
      source_document_path: sourceDocumentPath ?? null,
      source_manifest_path: sourceManifest?.manifest_path ?? null,
      workspace_id: sourceManifest?.workspace_id ?? null,
      source_id: sourceManifest?.source_id ?? null,
      skip_graph_generation: options.skipGraphGeneration ? true : null,
    },
  };
  const response = options.useRuntime === false
    ? await runOneShotEngineCommand("compile_project", request)
    : await runEngineCommand("compile_project", request);
  return {
    projectId: response.data.project_id,
    workspaceId: response.data.workspace_id,
    sourceId: response.data.source_id,
    graphGenerationStatus: response.data.graph_generation_status ?? null,
    graphGenerationSkippedReason: response.data.graph_generation_skipped_reason ?? null,
    graphGenerationErrorMessage: response.data.graph_generation_error_message ?? null,
  };
}

function enqueueWorkspaceGraphRebuild(
  sourceMarkdownPath,
  sourceDocumentPath,
  sourceManifest,
  activeJobId,
) {
  graphRebuildQueue = graphRebuildQueue
    .catch(() => {})
    .then(() =>
      runWorkspaceGraphRebuild(sourceMarkdownPath, sourceDocumentPath, sourceManifest, activeJobId),
    );
}

async function runWorkspaceGraphRebuild(
  sourceMarkdownPath,
  sourceDocumentPath,
  sourceManifest,
  activeJobId,
) {
  updateActiveGraphRebuildJob(activeJobId, {
    status: "citation_ready",
    progressPercent: 96,
    lastMessage: "Preparing context graph",
  });
  pushProgressEntry("graph", `Rebuilding workspace graph for ${sourceManifest.output_name}`);
  publishSnapshot();
  try {
    const project = await compileWorkspaceProject(
      sourceMarkdownPath,
      sourceDocumentPath,
      sourceManifest,
      { skipGraphGeneration: false, useRuntime: false },
    );
    snapshot.lastProjectId = project.projectId;
    snapshot.lastWorkspaceId = project.workspaceId ?? snapshot.lastWorkspaceId;
    snapshot.lastSourceId = project.sourceId ?? snapshot.lastSourceId;
    if (isGraphGenerationBlockingFailure(project.graphGenerationStatus)) {
      const message = graphGenerationFailureMessage(project);
      pushProgressEntry("graph", message);
      updateActiveGraphRebuildJob(activeJobId, {
        status: "failed",
        progressPercent: 100,
        lastMessage: message,
      });
      publishSnapshot();
      clearActiveGraphRebuildJob(activeJobId);
      return;
    }
    snapshot.workspaceRevision += 1;
    pushProgressEntry(
      "graph",
      graphGenerationNonBlockingMessage(project) ?? "Workspace graph rebuild completed",
    );
    updateActiveGraphRebuildJob(activeJobId, {
      status: "context_ready",
      progressPercent: 100,
      lastMessage: "Workspace graph rebuild completed",
    });
    publishSnapshot();
    clearActiveGraphRebuildJob(activeJobId);
  } catch (error) {
    pushProgressEntry("graph", `Workspace graph rebuild failed: ${error.message}`);
    updateActiveGraphRebuildJob(activeJobId, {
      status: "failed",
      progressPercent: 100,
      lastMessage: `Workspace graph rebuild failed: ${error.message}`,
    });
    publishSnapshot();
    clearActiveGraphRebuildJob(activeJobId);
  }
}

function updateActiveGraphRebuildJob(activeJobId, patch) {
  if (!activeJobId || snapshot.activeJob?.jobId !== activeJobId) {
    return;
  }
  snapshot.activeJob = {
    ...snapshot.activeJob,
    ...patch,
  };
}

function clearActiveGraphRebuildJob(activeJobId) {
  if (!activeJobId || snapshot.activeJob?.jobId !== activeJobId) {
    return;
  }
  snapshot.activeJob = null;
  publishSnapshot();
}

function isGraphGenerationBlockingFailure(status) {
  return status === "failed";
}

function graphGenerationFailureMessage(project) {
  if (project.graphGenerationErrorMessage) {
    return `Knowledge graph generation failed: ${project.graphGenerationErrorMessage}`;
  }
  if (project.graphGenerationSkippedReason) {
    return `Knowledge graph generation skipped: ${project.graphGenerationSkippedReason}`;
  }
  return `Knowledge graph generation failed with status: ${project.graphGenerationStatus}`;
}

function graphGenerationNonBlockingMessage(project) {
  if (!project.graphGenerationStatus) {
    return null;
  }
  if (project.graphGenerationStatus === "skipped") {
    return project.graphGenerationSkippedReason
      ? `Knowledge graph generation skipped: ${project.graphGenerationSkippedReason}`
      : "Knowledge graph generation skipped";
  }
  if (project.graphGenerationStatus === "empty") {
    return "Knowledge graph generation completed with no workspace changes";
  }
  if (project.graphGenerationStatus === "rebuilt") {
    return "Workspace graph rebuild completed";
  }
  if (project.graphGenerationStatus === "partially_applied") {
    return project.graphGenerationErrorMessage
      ? `Knowledge graph generation partially applied: ${project.graphGenerationErrorMessage}`
      : "Knowledge graph generation partially applied";
  }
  return null;
}

function runEngineCommand(expectedCommand, request, options = {}) {
  const binarySignature = engineBinarySignature();
  if (
    engineRuntime &&
    engineRuntimeBinarySignature &&
    binarySignature !== engineRuntimeBinarySignature
  ) {
    resetEngineRuntime();
  }
  if (!engineRuntime) {
    engineRuntime = new EngineRuntime({ spawnEngine: spawnEngineProcess });
    engineRuntimeBinarySignature = binarySignature;
  }
  return engineRuntime.run(expectedCommand, request, options);
}

function resetEngineRuntime() {
  if (engineRuntime) {
    engineRuntime.stop();
    engineRuntime = null;
  }
  engineRuntimeBinarySignature = null;
}

function engineBinarySignature() {
  const enginePath = resolveEnginePath();
  try {
    const stat = fs.statSync(enginePath);
    return `${enginePath}:${stat.size}:${stat.mtimeMs}`;
  } catch {
    return enginePath;
  }
}

function runOneShotEngineCommand(expectedCommand, request) {
  return runOneShotEngineRequest(expectedCommand, request, spawnEngineProcess);
}

function spawnEngineProcess(args) {
  return spawn(resolveEnginePath(), args, {
    stdio: ["pipe", "pipe", "pipe"],
    env: engineEnvironment(),
  });
}

function applyProgressEvent(event) {
  if (!snapshot.activeJob) {
    return;
  }
  snapshot.activeJob.status = "parsing";
  switch (event.type) {
    case "queued":
      snapshot.activeJob.progressPercent = 6;
      snapshot.activeJob.lastMessage = "Queued parse request";
      pushProgressEntry("queued", "Queued parse request");
      break;
    case "document_opened":
      snapshot.activeJob.progressPercent = 12;
      snapshot.activeJob.lastMessage = `Opened ${event.format}`;
      pushProgressEntry("opened", `Opened ${event.format}`);
      break;
    case "converting_pages":
      snapshot.activeJob.progressPercent = scaledProgress(event.current, event.total, 15, 48);
      snapshot.activeJob.lastMessage = `Preparing page ${event.current} of ${event.total}`;
      pushProgressEntry("converting", snapshot.activeJob.lastMessage);
      break;
    case "parsing":
      snapshot.activeJob.progressPercent = scaledProgress(event.current, event.total, 48, 88);
      snapshot.activeJob.lastMessage = `Parsing page ${event.current} of ${event.total}`;
      pushProgressEntry("parsing", snapshot.activeJob.lastMessage);
      break;
    case "packaging":
      snapshot.activeJob.status = "packaging";
      snapshot.activeJob.progressPercent = 94;
      snapshot.activeJob.lastMessage = "Saving markdown package";
      pushProgressEntry("packaging", "Saving markdown package");
      break;
    case "completed":
      snapshot.activeJob.status = "packaging";
      snapshot.activeJob.progressPercent = 100;
      snapshot.activeJob.lastMessage = "Parse completed";
      pushProgressEntry("completed", "Parse completed");
      break;
    case "failed":
      snapshot.activeJob.status = "failed";
      snapshot.activeJob.lastMessage = event.message;
      pushProgressEntry("failed", event.message);
      break;
  }
  publishSnapshot();
}

function markFailed(message) {
  if (snapshot.activeJob) {
    snapshot.activeJob.status = "failed";
    snapshot.activeJob.progressPercent = 100;
    snapshot.activeJob.lastMessage = message;
  }
  pushProgressEntry("failed", message);
  publishSnapshot();
  snapshot.activeJob = null;
}

function publishSnapshot() {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send(SNAPSHOT_EVENT, snapshot);
  }
}

function pushProgressEntry(phase, message) {
  snapshot.progressLog.unshift({
    phase,
    message,
    timestamp: String(Math.floor(Date.now() / 1000)),
  });
  snapshot.progressLog = snapshot.progressLog.slice(0, MAX_PROGRESS_LOG);
}

function scaledProgress(current, total, start, end) {
  if (!total) return start;
  const pct = Math.max(0, Math.min(1, current / total));
  return start + Math.round((end - start) * pct);
}

function nextJobId() {
  return `job-${Date.now()}`;
}

function detectFormat(filePath) {
  const ext = path.extname(filePath).slice(1).toLowerCase();
  if (["pdf", "docx", "doc"].includes(ext)) return ext;
  if (["png", "jpg", "jpeg", "webp"].includes(ext)) return "image";
  return null;
}

function formatForEngine(format) {
  switch (String(format).toLowerCase()) {
    case "pdf":
      return "pdf";
    case "docx":
      return "docx";
    case "doc":
      return "doc";
    case "image":
      return "image";
    default:
      throw new Error(`unsupported format: ${format}`);
  }
}

async function getModelsForProvider(providerSlug) {
  const catalog = await getProviderModelCatalog();
  const engineModels = catalog.provider_models?.[providerSlug] ?? [];
  if (providerSlug === "ollama") {
    const localModels = await listOllamaVisionModels(catalog.ollama_vision_prefixes ?? []);
    if (localModels.length > 0) {
      return localModels;
    }
  }
  return engineModels;
}

function getProviderModelCatalog() {
  if (!providerModelCatalogPromise) {
    providerModelCatalogPromise = runEngineCommand("list_provider_models", {
      command: "list_provider_models",
      payload: {},
    }).then((response) => response.data);
  }
  return providerModelCatalogPromise;
}

function listOllamaVisionModels(visionPrefixes) {
  return new Promise((resolve) => {
    const request = http.get("http://127.0.0.1:11434/v1/models", { timeout: 3000 }, (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => {
        body += chunk;
      });
      response.on("end", () => {
        try {
          const json = JSON.parse(body);
          const models = [];
          for (const entry of json.data ?? []) {
            const id = entry.id;
            if (
              typeof id === "string" &&
              visionPrefixes.some((prefix) => id.startsWith(prefix))
            ) {
              if (!models.includes(id)) {
                models.push(id);
              }
            }
          }
          resolve(models);
        } catch {
          resolve([]);
        }
      });
    });
    request.on("timeout", () => {
      request.destroy();
      resolve([]);
    });
    request.on("error", () => resolve([]));
  });
}

async function maybeImportLegacySwiftConfig() {
  await importLegacySwiftConfig(app, runEngineCommand);
}

function resolveEnginePath() {
  if (process.env.HYPRDUCK_ENGINE_BIN) {
    return process.env.HYPRDUCK_ENGINE_BIN;
  }

  if (!app.isPackaged) {
    const devTargetPath = path.join(
      __dirname,
      "..",
      "..",
      "target",
      "debug",
      process.platform === "win32" ? "hyprduck-engine.exe" : "hyprduck-engine",
    );
    if (fs.existsSync(devTargetPath)) {
      return devTargetPath;
    }
  }

  const engineName = `hyprduck-engine-${hostTriple()}`;
  const devPath = path.join(__dirname, "resources", "binaries", engineName);
  if (fs.existsSync(devPath)) {
    return devPath;
  }

  const packagedPath = path.join(process.resourcesPath, "binaries", engineName);
  if (fs.existsSync(packagedPath)) {
    return packagedPath;
  }

  return "hyprduck-engine";
}
