const { execFile } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

async function maybeImportLegacySwiftConfig(app, runEngineCommand) {
  if (fs.existsSync(engineConfigPath(app))) {
    return;
  }
  const payload = await readLegacySwiftPayload(app);
  if (!payload) {
    return;
  }
  await runEngineCommand("save_config", {
    command: "save_config",
    payload: { config: payload },
  });
}

function engineConfigPath(app) {
  if (process.env.ETYMA_CONFIG_DIR) {
    return path.join(process.env.ETYMA_CONFIG_DIR, "engine-config.json");
  }
  return path.join(app.getPath("home"), ".etyma", "engine-config.json");
}

async function readLegacySwiftPayload(app) {
  for (const plistPath of legacyPreferencePaths(app)) {
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

function legacyPreferencePaths(app) {
  const home = app.getPath("home");
  return [
    path.join(home, "Library", "Preferences", "app.Etyma.plist"),
    path.join(home, "Library", "Preferences", "Etyma.plist"),
  ];
}

async function legacyPayloadFromPlist(plistPath) {
  const providerBlob = await plutilExtractRaw(plistPath, "ai_provider_config");
  const templateBlob = await plutilExtractRaw(plistPath, "selected_prompt_template");
  const legacyProvider = providerBlob ? parseLegacyProviderBlob(providerBlob) : {};

  if (!legacyProvider.providerType && !templateBlob) {
    return null;
  }

  const provider = engineProviderSlug(legacyProvider.providerType);
  const supportedProvider = provider ?? "open_router";
  const promptTemplate = templateBlob ? parseLegacyTemplateBlob(templateBlob) : "General";
  const apiKey = provider
    ? await selectLegacyApiKey(plistPath, provider, legacyProvider.apiKey)
    : "";

  return {
    provider: supportedProvider,
    model_id: provider
      ? legacyProvider.modelId ?? defaultModelForProvider(provider)
      : defaultModelForProvider(supportedProvider),
    api_key: apiKey,
    base_url: provider ? legacyProvider.baseUrl ?? null : null,
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
      ? "com.etyma.openrouter"
      : providerSlug === "ollama"
        ? "com.etyma.ollama"
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
    default:
      return "openai/gpt-4.1-mini";
  }
}

module.exports = {
  maybeImportLegacySwiftConfig,
};
