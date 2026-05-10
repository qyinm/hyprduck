const { app, BrowserWindow, dialog, ipcMain, shell } = require("electron");
const { spawn, execFile } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");

const SNAPSHOT_EVENT = "duckdocs://snapshot";
const MAX_PROGRESS_LOG = 80;

const snapshot = {
  activeJob: null,
  progressLog: [],
  lastResult: null,
  lastProjectId: null,
};

let mainWindow = null;
let engineRuntime = null;
let providerModelCatalogPromise = null;

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
  ipcMain.handle("duckdocs:invoke", async (_event, command, args = {}) => {
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
      case "get_models_for_provider":
        return getModelsForProvider(args.providerSlug);
      case "load_workspace_project":
        return runEngineCommand("load_project", {
          command: "load_project",
          payload: { project_id: args.project_id ?? null },
        }).then((response) => response.data.project ?? null);
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
        return openSavedOutput(args.path, Boolean(args.reveal));
      default:
        throw new Error(`unknown HyprDuck command: ${command}`);
    }
  });
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
    pushProgressEntry(
      "completed",
      data.saved_output_path ?? "Parse completed without a saved output path",
    );

    if (data.saved_output_path) {
      try {
        const projectId = await compileWorkspaceProject(data.saved_output_path, request.path);
        snapshot.lastProjectId = projectId;
        pushProgressEntry("compile", `Compiled knowledge workspace ${projectId}`);
      } catch (error) {
        snapshot.lastProjectId = null;
        pushProgressEntry("compile_failed", `Knowledge compile failed: ${error.message}`);
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
    applyProgressEvent(JSON.parse(line));
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
      this.failRuntime(`failed to spawn duckdocs-engine: ${error.message}`);
    });
    child.on("close", (code) => {
      const message = this.stopping
        ? "engine runtime stopped"
        : `duckdocs-engine runtime exited${code === null ? "" : ` with status ${code}`}`;
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
          active.onEvent?.(JSON.stringify(message.event));
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
      } else if (response.type === "event") {
        active.onEvent?.(JSON.stringify(response.event));
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
    DUCKDOCS_OUTPUT_DIR: ensureHyprduckApplicationSupportPath(),
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

async function compileWorkspaceProject(sourceMarkdownPath, sourceDocumentPath) {
  const response = await runEngineCommand("compile_project", {
    command: "compile_project",
    payload: {
      source_markdown_path: sourceMarkdownPath,
      source_document_path: sourceDocumentPath ?? null,
    },
  });
  return response.data.project_id;
}

function runEngineCommand(expectedCommand, request, options = {}) {
  if (!engineRuntime) {
    engineRuntime = new EngineRuntime();
  }
  return engineRuntime.run(expectedCommand, request, options);
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
  snapshot.activeJob = null;
  pushProgressEntry("failed", message);
  publishSnapshot();
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
      return "Pdf";
    case "docx":
      return "Docx";
    case "doc":
      return "Doc";
    case "image":
      return "Image";
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
  if (process.env.DUCKDOCS_ENGINE_BIN) {
    return process.env.DUCKDOCS_ENGINE_BIN;
  }

  const sidecarName = `duckdocs-engine-${hostTriple()}`;
  const devPath = path.join(__dirname, "resources", "binaries", sidecarName);
  if (fs.existsSync(devPath)) {
    return devPath;
  }

  const packagedPath = path.join(process.resourcesPath, "binaries", sidecarName);
  if (fs.existsSync(packagedPath)) {
    return packagedPath;
  }

  return "duckdocs-engine";
}

function hostTriple() {
  if (process.env.DUCKDOCS_TARGET_TRIPLE) {
    return process.env.DUCKDOCS_TARGET_TRIPLE;
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
  if (process.env.DUCKDOCS_CONFIG_DIR) {
    return path.join(process.env.DUCKDOCS_CONFIG_DIR, "engine-config.json");
  }
  return path.join(app.getPath("home"), ".duckdocs", "engine-config.json");
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
    path.join(home, "Library", "Preferences", "app.DuckDocs.plist"),
    path.join(home, "Library", "Preferences", "DuckDocs.plist"),
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
      ? "com.duckdocs.openrouter"
      : providerSlug === "ollama"
        ? "com.duckdocs.ollama"
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
