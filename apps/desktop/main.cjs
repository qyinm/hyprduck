const { app, BrowserWindow, dialog, ipcMain, shell } = require("electron");
const { spawn, execFile } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");

const SNAPSHOT_EVENT = "hyprduck://snapshot";
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
}

app.whenReady().then(async () => {
  registerIpcHandlers();
  try {
    await maybeImportLegacySwiftConfig();
  } catch (error) {
    console.error("legacy config migration skipped:", error);
  }
  createWindow();
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

function registerIpcHandlers() {
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
      case "resolve_brain_review": {
        const workspaceId = args.workspace_id ?? snapshot.lastWorkspaceId ?? "default";
        return runEngineCommand("resolve_brain_review_item", {
          command: "resolve_brain_review_item",
          payload: {
            scope: brainReadScope(workspaceId),
            proposalId: args.proposal_id,
            decision: args.decision,
            actor: {
              actorType: "user",
              actorId: "local-user",
            },
            reason: args.reason ?? null,
          },
        }).then((response) => response.data);
      }
      case "propose_brain_update": {
        const workspaceId = args.workspace_id ?? snapshot.lastWorkspaceId ?? "default";
        return runEngineCommand("propose_brain_update", {
          command: "propose_brain_update",
          payload: {
            scope: brainReadScope(workspaceId),
            kind: args.kind,
            title: args.title,
            body: args.body,
            actor: {
              actorType: "user",
              actorId: "local-user",
            },
            targetNodeId: args.target_node_id ?? null,
            targetSourceId: args.target_source_id ?? null,
            relationKind: args.relation_kind ?? null,
            sourceDescription: args.source_description ?? null,
            sourceUserContext: args.source_user_context ?? null,
            sourceIngestInstruction: args.source_ingest_instruction ?? null,
            sourceRefs: args.source_refs ?? [],
            nodeRefs: args.node_refs ?? [],
            evidenceRefs: args.evidence_refs ?? [],
            proposalPayload: args.proposal_payload ?? null,
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
      case "start_parse":
        return startParse(args.request);
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
    status: "queued",
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
          snapshot.activeJob.progressPercent = 100;
          snapshot.activeJob.lastMessage = "Compiling knowledge workspace";
          pushProgressEntry("compile", "Compiling knowledge workspace");
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
        let graphRebuildQueued = false;
        if (sourceManifest) {
          graphRebuildQueued = true;
          if (snapshot.activeJob) {
            snapshot.activeJob.status = "running";
            snapshot.activeJob.progressPercent = 96;
            snapshot.activeJob.lastMessage = "Rebuilding workspace graph";
          }
          pushProgressEntry("graph", "Queued workspace graph rebuild");
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

function applyRuntimeProgressLine(line) {
  try {
    const event = typeof line === "string" ? JSON.parse(line) : line;
    applyProgressEvent(event);
  } catch {
    // Non-event stderr is ignored; engine failures still arrive on stdout.
  }
}

class EngineRuntime {
  constructor() {
    this.child = null;
    this.stdoutBuffer = "";
    this.stderrBuffer = "";
    this.active = null;
    this.queue = [];
    this.stopping = false;
  }

  run(expectedCommand, request, options = {}) {
    return new Promise((resolve, reject) => {
      this.queue.push({
        id: uuidv7(),
        expectedCommand,
        request,
        onEvent: options.onEvent ?? null,
        resolve,
        reject,
      });
      this.pump();
    });
  }

  pump() {
    if (this.active || this.queue.length === 0) {
      return;
    }
    try {
      this.ensureStarted();
    } catch (error) {
      const next = this.queue.shift();
      next?.reject(error);
      return;
    }
    this.active = this.queue.shift();
    try {
      this.child.stdin.write(
        `${JSON.stringify({ id: this.active.id, ...this.active.request })}\n`,
      );
    } catch (error) {
      const active = this.active;
      this.active = null;
      active?.reject(new Error(`failed writing engine request: ${error.message}`));
      this.failRuntime("engine runtime stdin is unavailable");
      this.pump();
    }
  }

  ensureStarted() {
    if (this.child && !this.child.killed) {
      return;
    }
    this.stopping = false;
    const child = spawn(resolveEnginePath(), ["serve"], {
      stdio: ["pipe", "pipe", "pipe"],
      env: engineEnvironment(),
    });
    this.child = child;
    this.stdoutBuffer = "";
    this.stderrBuffer = "";

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => this.handleStdout(chunk));
    child.stderr.on("data", (chunk) => this.handleStderr(chunk));
    child.on("error", (error) => {
      if (this.child !== child) {
        return;
      }
      this.failRuntime(`failed to spawn hyprduck-engine: ${error.message}`);
    });
    child.on("close", (code) => {
      if (this.child !== child) {
        return;
      }
      const message = this.stopping
        ? "engine runtime stopped"
        : `hyprduck-engine runtime exited${code === null ? "" : ` with status ${code}`}`;
      this.child = null;
      this.stdoutBuffer = "";
      this.stderrBuffer = "";
      if (!this.stopping) {
        this.failRuntime(message);
      }
    });
  }

  handleStdout(chunk) {
    this.stdoutBuffer += chunk;
    const lines = this.stdoutBuffer.split(/\r?\n/);
    this.stdoutBuffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) {
        continue;
      }
      this.completeActive(line);
    }
  }

  handleStderr(chunk) {
    this.stderrBuffer += chunk;
    const lines = this.stderrBuffer.split(/\r?\n/);
    this.stderrBuffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) {
        continue;
      }
      this.handleRuntimeEvent(line);
    }
  }

  handleRuntimeEvent(line) {
    const active = this.active;
    if (!active) {
      return;
    }
    try {
      const message = JSON.parse(line);
      if (message.type === "event") {
        if (message.id === active.id) {
          active.onEvent?.(message.event);
        }
        return;
      }
    } catch {
      // Legacy one-shot engine mode writes raw parse events to stderr.
    }
    active.onEvent?.(line);
  }

  completeActive(line) {
    const active = this.active;
    if (!active) {
      return;
    }
    try {
      const response = JSON.parse(line);
      if (response.id !== active.id) {
        active.reject(
          new Error(`engine response id mismatch: expected ${active.id}, got ${response.id}`),
        );
        this.active = null;
        this.stop();
        return;
      } else if (response.type === "event") {
        active.onEvent?.(response.event);
        return;
      } else if (response.ok === false) {
        active.reject(new Error(response.error?.message ?? "engine command failed"));
        this.active = null;
      } else if (response.command !== active.expectedCommand) {
        active.reject(
          new Error(
            `engine response command mismatch: expected ${active.expectedCommand}, got ${response.command}`,
          ),
        );
        this.active = null;
      } else {
        active.resolve(response);
        this.active = null;
      }
    } catch (error) {
      active.reject(new Error(`failed decoding engine response: ${error.message}`));
      this.active = null;
    }
    this.pump();
  }

  failRuntime(message) {
    const error = new Error(message);
    if (this.active) {
      this.active.reject(error);
      this.active = null;
    }
    while (this.queue.length > 0) {
      this.queue.shift().reject(error);
    }
  }

  stop() {
    this.stopping = true;
    if (this.child) {
      this.child.kill();
      this.child = null;
    }
    this.failRuntime("engine runtime stopped");
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
    status: "running",
    progressPercent: 96,
    lastMessage: "Rebuilding workspace graph",
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
      status: "completed",
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
    engineRuntime = new EngineRuntime();
  }
  return engineRuntime.run(expectedCommand, request, options);
}

function runOneShotEngineCommand(expectedCommand, request) {
  return new Promise((resolve, reject) => {
    const child = spawn(resolveEnginePath(), [], {
      stdio: ["pipe", "pipe", "pipe"],
      env: engineEnvironment(),
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      reject(new Error(`failed to spawn hyprduck-engine: ${error.message}`));
    });
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(lastNonEmptyLine(stderr) ?? `hyprduck-engine exited with status ${code}`));
        return;
      }
      try {
        const response = JSON.parse(stdout);
        if (response.ok === false) {
          reject(new Error(response.error?.message ?? "engine command failed"));
          return;
        }
        if (response.command !== expectedCommand) {
          reject(
            new Error(
              `engine response command mismatch: expected ${expectedCommand}, got ${response.command}`,
            ),
          );
          return;
        }
        resolve(response);
      } catch (error) {
        reject(new Error(`failed decoding engine response: ${error.message}`));
      }
    });
    child.stdin.end(JSON.stringify(request));
  });
}

function lastNonEmptyLine(value) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .pop();
}

function uuidv7() {
  const bytes = crypto.randomBytes(16);
  let timestamp = BigInt(Date.now());
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(timestamp & 0xffn);
    timestamp >>= 8n;
  }
  bytes[6] = 0x70 | (bytes[6] & 0x0f);
  bytes[8] = 0x80 | (bytes[8] & 0x3f);
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function applyProgressEvent(event) {
  if (!snapshot.activeJob) {
    return;
  }
  snapshot.activeJob.status = "running";
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
      snapshot.activeJob.progressPercent = 94;
      snapshot.activeJob.lastMessage = "Saving markdown package";
      pushProgressEntry("packaging", "Saving markdown package");
      break;
    case "completed":
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

function resolveEnginePath() {
  if (process.env.HYPRDUCK_ENGINE_BIN) {
    return process.env.HYPRDUCK_ENGINE_BIN;
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

function hostTriple() {
  if (process.env.HYPRDUCK_TARGET_TRIPLE) {
    return process.env.HYPRDUCK_TARGET_TRIPLE;
  }
  if (process.platform === "darwin" && process.arch === "arm64") return "aarch64-apple-darwin";
  if (process.platform === "darwin" && process.arch === "x64") return "x86_64-apple-darwin";
  if (process.platform === "linux" && process.arch === "x64") return "x86_64-unknown-linux-gnu";
  if (process.platform === "win32" && process.arch === "x64") return "x86_64-pc-windows-msvc";
  return `${process.arch}-${process.platform}`;
}

async function maybeImportLegacySwiftConfig() {
  if (fs.existsSync(engineConfigPath())) {
    return;
  }
  const payload = await readLegacySwiftPayload();
  if (!payload) {
    return;
  }
  await runEngineCommand("save_config", {
    command: "save_config",
    payload: { config: payload },
  });
}

function engineConfigPath() {
  if (process.env.HYPRDUCK_CONFIG_DIR) {
    return path.join(process.env.HYPRDUCK_CONFIG_DIR, "engine-config.json");
  }
  return path.join(app.getPath("home"), ".hyprduck", "engine-config.json");
}

async function readLegacySwiftPayload() {
  for (const plistPath of legacyPreferencePaths()) {
    if (!fs.existsSync(plistPath)) {
      continue;
    }
    const payload = await legacyPayloadFromPlist(plistPath);
    if (payload) {
      return payload;
    }
  }
  return null;
}

function legacyPreferencePaths() {
  const home = app.getPath("home");
  return [
    path.join(home, "Library", "Preferences", "app.HyprDuck.plist"),
    path.join(home, "Library", "Preferences", "HyprDuck.plist"),
  ];
}

async function legacyPayloadFromPlist(plistPath) {
  const providerBlob = await plutilExtractRaw(plistPath, "ai_provider_config");
  const templateBlob = await plutilExtractRaw(plistPath, "selected_prompt_template");
  const legacyProvider = providerBlob ? parseLegacyProviderBlob(providerBlob) : {};

  if (!legacyProvider.providerType && !templateBlob) {
    return null;
  }

  const provider = engineProviderSlug(legacyProvider.providerType) ?? "open_router";
  const promptTemplate = templateBlob ? parseLegacyTemplateBlob(templateBlob) : "General";
  const apiKey = await selectLegacyApiKey(plistPath, provider, legacyProvider.apiKey);

  return {
    provider,
    model_id: legacyProvider.modelId ?? defaultModelForProvider(provider),
    api_key: apiKey,
    base_url: legacyProvider.baseUrl ?? null,
    prompt_template: promptTemplate,
    provider_options: [],
    model_options: [],
    prompt_template_options: [],
  };
}

async function selectLegacyApiKey(plistPath, providerSlug, embeddedApiKey) {
  if (embeddedApiKey?.trim()) {
    return embeddedApiKey;
  }
  const defaultsKey =
    providerSlug === "open_router"
      ? "openrouter_api_key"
      : providerSlug === "ollama"
        ? "ollama_api_key"
        : null;
  if (defaultsKey) {
    const defaultsValue = await plutilExtractRaw(plistPath, defaultsKey);
    if (defaultsValue?.trim()) {
      return defaultsValue;
    }
  }
  const keychainValue = await legacyApiKeyFromKeychain(providerSlug);
  return keychainValue?.trim() ? keychainValue : "";
}

function plutilExtractRaw(plistPath, key) {
  return execFileText("/usr/bin/plutil", ["-extract", key, "raw", "-o", "-", plistPath]).catch(
    () => null,
  );
}

async function legacyApiKeyFromKeychain(providerSlug) {
  const service =
    providerSlug === "open_router"
      ? "com.hyprduck.openrouter"
      : providerSlug === "ollama"
        ? "com.hyprduck.ollama"
        : null;
  if (!service) {
    return null;
  }
  return execFileText("/usr/bin/security", [
    "find-generic-password",
    "-s",
    service,
    "-a",
    "apikey",
    "-w",
  ]).catch(() => null);
}

function execFileText(command, args) {
  return new Promise((resolve, reject) => {
    execFile(command, args, { encoding: "utf8" }, (error, stdout) => {
      if (error) {
        reject(error);
        return;
      }
      const value = stdout.trim();
      resolve(value || null);
    });
  });
}

function parseLegacyProviderBlob(blob) {
  return JSON.parse(Buffer.from(blob, "base64").toString("utf8"));
}

function parseLegacyTemplateBlob(blob) {
  return JSON.parse(Buffer.from(blob, "base64").toString("utf8"));
}

function engineProviderSlug(value) {
  switch (value) {
    case "OpenRouter":
      return "open_router";
    case "OpenAI":
      return "open_ai";
    case "Anthropic":
      return "anthropic";
    case "Ollama":
      return "ollama";
    default:
      return null;
  }
}

function defaultModelForProvider(providerSlug) {
  switch (providerSlug) {
    case "ollama":
      return "qwen3-vl:8b";
    case "open_ai":
      return "gpt-4.1-mini";
    case "anthropic":
      return "claude-3-5-sonnet-20241022";
    default:
      return "openai/gpt-4.1-mini";
  }
}
