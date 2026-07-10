const { app, BrowserWindow, net, protocol, shell } = require("electron");
const path = require("node:path");
const { ensureHyprduckShellCommand } = require("./main/cli-shim.cjs");
const { createEngineRpc } = require("./main/engine-rpc.cjs");
const { createSnapshotState } = require("./main/snapshot-state.cjs");
const { createImportPipeline } = require("./main/import-pipeline.cjs");
const {
  registerSourcePreviewScheme,
  createSourcePreview,
} = require("./main/source-preview.cjs");
const { createAgentChatStream } = require("./main/agent-chat-stream.cjs");
const { registerIpcHandlers } = require("./main/ipc-handlers.cjs");

registerSourcePreviewScheme(protocol);

let mainWindow = null;
let autoUpdateStarted = false;

const getMainWindow = () => mainWindow;

const {
  snapshot,
  publishSnapshot,
  pushProgressEntry,
  markFailed,
  applyRuntimeProgressLine,
  nextJobId,
} = createSnapshotState({ getMainWindow });

const {
  brainReadScope,
  ensureHyprduckApplicationSupportPath,
  getModelsForProvider,
  maybeImportLegacySwiftConfig,
  resetEngineRuntime,
  runEngineCommand,
  runOneShotEngineCommand,
} = createEngineRpc({ app });

const {
  registerSourcePreviewProtocol,
  resolveKnownWorkspacePath,
  readSourceDetail,
  openLocalArtifact,
} = createSourcePreview({
  app,
  protocol,
  net,
  shell,
  ensureHyprduckApplicationSupportPath,
});

const {
  applyWorkspaceCorrection,
  startParse,
  retryFailedPages,
  cancelParse,
  detectFormat,
} = createImportPipeline({
  snapshot,
  pushProgressEntry,
  publishSnapshot,
  markFailed,
  applyRuntimeProgressLine,
  nextJobId,
  runEngineCommand,
  runOneShotEngineCommand,
  resetEngineRuntime,
  ensureHyprduckApplicationSupportPath,
  resolveKnownWorkspacePath,
});

const { startAgentChat, stopAgentChat } = createAgentChatStream({
  getMainWindow,
  snapshot,
  runEngineCommand,
  resetEngineRuntime,
  brainReadScope,
});

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

app.whenReady().then(async () => {
  registerSourcePreviewProtocol();
  await registerIpcHandlers({
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
  });
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
