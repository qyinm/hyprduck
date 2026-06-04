const { app, BrowserWindow, dialog, ipcMain, shell } = require("electron");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { ensureHyprduckShellCommand, hostTriple } = require("./main/cli-shim.cjs");
const {
  EngineRuntime,
  runOneShotEngineCommand: runOneShotEngineRequest,
} = require("./main/engine-runtime.cjs");
const {
  maybeImportLegacySwiftConfig: importLegacySwiftConfig,
} = require("./main/legacy-config.cjs");
const { AgentTerminalSessionManager } = require("./main/agent-terminal-sessions.cjs");
const { DisabledAgentTerminalBackend } = require("./main/agent-terminal-backend.cjs");
const { createGhosttyNativeBackendFromEnv } = require("./main/agent-terminal-ghostty.cjs");

const SNAPSHOT_EVENT = "hyprduck://snapshot";
const AGENT_TERMINAL_EVENT = "hyprduck://agent-terminal";
const MAX_PROGRESS_LOG = 80;

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
let providerModelCatalogPromise = null;
let graphRebuildQueue = Promise.resolve();
let agentTerminalSessions = null;
let autoUpdateStarted = false;

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
  if (engineRuntime) {
    engineRuntime.stop();
    engineRuntime = null;
  }
});

async function registerIpcHandlers() {
  const ghosttyProbe = await createGhosttyNativeBackendFromEnv();
  const fallbackBackend = ghosttyProbe.enabled
    ? new DisabledAgentTerminalBackend({
        reason: ghosttyProbe.reason ?? "Ghostty native backend is unavailable.",
      })
    : new DisabledAgentTerminalBackend({
        reason:
          "Agent terminal backend is disabled in packaged builds until native PTY startup loading is isolated.",
      });
  agentTerminalSessions = new AgentTerminalSessionManager({
    backend: ghosttyProbe.backend ?? fallbackBackend,
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
      default:
        throw new Error(`unknown HyprDuck command: ${command}`);
    }
  });
}

function publishAgentTerminalEvent(payload) {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return;
  }
  mainWindow.webContents.send(AGENT_TERMINAL_EVENT, payload);
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
  if (engineRuntime) {
    engineRuntime.stop();
    engineRuntime = null;
  }
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
  if (!engineRuntime) {
    engineRuntime = new EngineRuntime({ spawnEngine: spawnEngineProcess });
  }
  return engineRuntime.run(expectedCommand, request, options);
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
