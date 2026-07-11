const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const {
  EngineRuntime,
  runOneShotEngineCommand: runOneShotEngineRequest,
} = require("./engine-runtime.cjs");
const {
  maybeImportLegacySwiftConfig: importLegacySwiftConfig,
} = require("./legacy-config.cjs");
const { hostTriple } = require("./cli-shim.cjs");

function createEngineRpc({ app }) {
  let engineRuntime = null;
  let engineRuntimeBinarySignature = null;
  let providerModelCatalogPromise = null;

  function etymaApplicationSupportPath() {
    const etyma = path.join(app.getPath("appData"), "Etyma");
    const legacy = path.join(app.getPath("appData"), "HyprDuck");
    // Prefer Etyma; keep reading legacy HyprDuck data during rename migration.
    if (fs.existsSync(etyma) || !fs.existsSync(legacy)) {
      return etyma;
    }
    return legacy;
  }

  function ensureEtymaApplicationSupportPath() {
    const storageRoot = etymaApplicationSupportPath();
    fs.mkdirSync(storageRoot, { recursive: true });
    return storageRoot;
  }

  function engineEnvironment() {
    const storageRoot = ensureEtymaApplicationSupportPath();
    return {
      ...process.env,
      ETYMA_OUTPUT_DIR: storageRoot,
      // Temporary alias so older tooling still finds the workspace root.
      HYPRDUCK_OUTPUT_DIR: storageRoot,
    };
  }

  function brainReadScope(workspaceId) {
    return {
      workspaceId,
      rootDir: ensureEtymaApplicationSupportPath(),
    };
  }

  function resolveEnginePath() {
    if (process.env.ETYMA_ENGINE_BIN) {
      return process.env.ETYMA_ENGINE_BIN;
    }

    if (!app.isPackaged) {
      const devTargetPath = path.join(
        __dirname,
        "..",
        "..",
        "..",
        "target",
        "debug",
        process.platform === "win32" ? "etyma-engine.exe" : "etyma-engine",
      );
      if (fs.existsSync(devTargetPath)) {
        return devTargetPath;
      }
    }

    const engineName = `etyma-engine-${hostTriple()}`;
    const devPath = path.join(__dirname, "..", "resources", "binaries", engineName);
    if (fs.existsSync(devPath)) {
      return devPath;
    }

    const packagedPath = path.join(process.resourcesPath, "binaries", engineName);
    if (fs.existsSync(packagedPath)) {
      return packagedPath;
    }

    return "etyma-engine";
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

  function spawnEngineProcess(args) {
    return spawn(resolveEnginePath(), args, {
      stdio: ["pipe", "pipe", "pipe"],
      env: engineEnvironment(),
    });
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

  function runOneShotEngineCommand(expectedCommand, request) {
    return runOneShotEngineRequest(expectedCommand, request, spawnEngineProcess);
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
      const request = http.get(
        "http://127.0.0.1:11434/v1/models",
        { timeout: 3000 },
        (response) => {
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
        },
      );
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

  return {
    brainReadScope,
    ensureEtymaApplicationSupportPath,
    getModelsForProvider,
    maybeImportLegacySwiftConfig,
    resetEngineRuntime,
    runEngineCommand,
    runOneShotEngineCommand,
  };
}

module.exports = {
  createEngineRpc,
};
