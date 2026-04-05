const tauri = window.__TAURI__;

if (!tauri) {
  throw new Error("DuckDocs desktop UI requires Tauri global APIs.");
}

const core = tauri.core;
const event = tauri.event;

const state = {
  snapshot: null,
  currentConfig: null,
  validation: null,
  selectedFile: null,
  activePanel: "import"
};

const root = document.querySelector("#app");

async function invoke(command, args = {}) {
  return core.invoke(command, args);
}

function formatStatus(kind) {
  switch (kind) {
    case "idle":
      return { label: "Idle", tone: "good" };
    case "queued":
      return { label: "Queued", tone: "warn" };
    case "running":
      return { label: "Running", tone: "warn" };
    case "completed":
      return { label: "Completed", tone: "good" };
    case "failed":
      return { label: "Failed", tone: "bad" };
    default:
      return { label: kind, tone: "" };
  }
}

function parseSummary(snapshot) {
  const result = snapshot?.lastResult;
  if (!result) {
    return "No completed imports yet.";
  }

  const counts = `${result.successCount} succeeded / ${result.failedCount} failed`;
  return result.savedOutputPath ? `${counts} · ${result.savedOutputPath}` : counts;
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function panelButton(id, label) {
  const active = state.activePanel === id ? "active" : "";
  return `<button class="ghost nav-pill ${active}" data-panel="${id}">${label}</button>`;
}

function renderPanel(snapshot) {
  switch (state.activePanel) {
    case "settings":
      return renderSettingsPanel();
    case "progress":
      return renderProgressPanel(snapshot);
    case "result":
      return renderResultPanel(snapshot);
    default:
      return renderImportPanel(snapshot);
  }
}

function renderImportPanel(snapshot) {
  const status = formatStatus(snapshot.activeJob?.status ?? "idle");
  const canStart = Boolean(state.selectedFile) && !snapshot.activeJob;

  return `
    <section class="dashboard">
      <article class="card">
        <h2>Import Dashboard</h2>
        <p class="muted">Choose a file, start parsing, and stay in one window while the job moves from config to progress to results.</p>
        <div class="file-drop">
          <strong>${state.selectedFile?.path ?? "No file selected"}</strong>
          <div class="muted">${state.selectedFile?.format?.toUpperCase?.() ?? "PDF, DOCX, and DOC are supported."}</div>
        </div>
        <div class="actions" style="margin-top: 16px;">
          <button id="choose-file">Choose File</button>
          <button id="start-parse" ${canStart ? "" : "disabled"}>Start Import</button>
          <button id="go-settings" class="secondary">Settings</button>
          <button id="go-progress" class="secondary">Progress</button>
          <button id="go-result" class="secondary">Result</button>
        </div>
        ${
          snapshot.activeJob
            ? `
              <div class="card inset-card">
                <h3>Current Job</h3>
                <div class="muted mono">${snapshot.activeJob.filePath}</div>
                <div class="progress-bar" style="margin-top: 12px;">
                  <span style="width: ${Math.max(6, snapshot.activeJob.progressPercent)}%;"></span>
                </div>
                <div class="status-line" style="margin-top: 12px;">
                  <span class="status-tag ${status.tone}">${status.label}</span>
                  <span class="muted">${snapshot.activeJob.lastMessage ?? "Waiting for engine events..."}</span>
                </div>
                <div class="actions" style="margin-top: 12px;">
                  <button id="cancel-parse" class="ghost">Cancel</button>
                </div>
              </div>
            `
            : `
              <div class="card inset-card">
                <h3>Latest Output</h3>
                <div class="muted">${parseSummary(snapshot)}</div>
                <div class="actions" style="margin-top: 12px;">
                  <button id="open-output" class="ghost" ${snapshot.lastResult?.savedOutputPath ? "" : "disabled"}>Open Markdown</button>
                  <button id="reveal-output" class="ghost" ${snapshot.lastResult?.savedOutputPath ? "" : "disabled"}>Reveal in Finder</button>
                </div>
              </div>
            `
        }
      </article>

      <aside class="card">
        <h2>Current State</h2>
        <div class="event-log">
          ${
            snapshot.progressLog.slice(0, 8).map((entry) => `
              <div class="event-row">
                <strong>${entry.phase}</strong>
                <div class="muted">${entry.message}</div>
              </div>
            `).join("") ||
            `<div class="event-row"><strong>No events yet</strong><div class="muted">Start an import to populate progress events.</div></div>`
          }
        </div>
      </aside>
    </section>
  `;
}

function renderSettingsPanel() {
  const config = state.currentConfig;
  const validation = state.validation;

  return `
    <section class="card panel-card">
      <div class="panel-heading">
        <div>
          <h2>AI Settings</h2>
          <p class="muted">Provider, model, API key, Ollama endpoint, and parsing template stay Rust-owned.</p>
        </div>
        <button id="back-to-import" class="ghost">Back to Import</button>
      </div>
      <form id="settings-form" class="field-grid">
        <label class="full">
          <span class="muted">Provider</span>
          <select name="provider">
            ${(config?.provider_options ?? []).map((option) => `<option value="${option.id}" ${option.id === config.provider ? "selected" : ""}>${option.label}</option>`).join("")}
          </select>
        </label>
        <label>
          <span class="muted">Model</span>
          <select name="model_id">
            ${(config?.model_options ?? []).map((model) => `<option value="${model}" ${model === config.model_id ? "selected" : ""}>${model}</option>`).join("")}
          </select>
        </label>
        <label>
          <span class="muted">Prompt Template</span>
          <select name="prompt_template">
            ${(config?.prompt_template_options ?? []).map((value) => `<option value="${value}" ${value === config.prompt_template ? "selected" : ""}>${value}</option>`).join("")}
          </select>
        </label>
        <label class="full">
          <span class="muted">API Key</span>
          <input type="password" name="api_key" value="${config?.api_key ?? ""}" autocomplete="off" />
        </label>
        <label class="full">
          <span class="muted">Base URL</span>
          <input type="text" name="base_url" value="${config?.base_url ?? ""}" placeholder="Optional custom endpoint" />
        </label>
        <div class="full actions">
          <button type="submit">Save Settings</button>
          <button type="button" id="validate-settings" class="secondary">Validate</button>
        </div>
      </form>
      <div class="notice-stack" style="margin-top: 18px;">
        ${
          validation
            ? `<div class="notice ${validation.ready ? "good" : "warn"}"><strong>${validation.ready ? "Ready to import" : "Needs attention"}</strong><div class="muted">${validation.issues.map((issue) => issue.message).join(" ") || "Validation passed."}</div></div>`
            : `<div class="notice"><strong>Validation pending.</strong><div class="muted">Save or validate the current engine configuration.</div></div>`
        }
      </div>
    </section>
  `;
}

function renderProgressPanel(snapshot) {
  return `
    <section class="card panel-card">
      <div class="panel-heading">
        <div>
          <h2>Progress</h2>
          <p class="muted">Parse events stream from the Rust engine stderr channel into the shared desktop store.</p>
        </div>
        <button id="back-to-import" class="ghost">Back to Import</button>
      </div>
      <div class="event-log">
        ${
          snapshot.progressLog.map((entry) => `
            <div class="event-row">
              <strong>${entry.phase}</strong>
              <div class="muted">${entry.message}</div>
              <div class="muted mono">${entry.timestamp}</div>
            </div>
          `).join("") ||
          `<div class="event-row"><strong>No active parse</strong><div class="muted">Run an import from the main view to inspect progress here.</div></div>`
        }
      </div>
    </section>
  `;
}

function renderResultPanel(snapshot) {
  const result = snapshot.lastResult;

  return `
    <section class="card panel-card">
      <div class="panel-heading">
        <div>
          <h2>Result</h2>
          <p class="muted">Saved markdown package metadata and a preview of the generated output.</p>
        </div>
        <button id="back-to-import" class="ghost">Back to Import</button>
      </div>
      ${
        result
          ? `
            <div class="result-grid">
              <div class="result-row">
                <strong>Saved Output</strong>
                <div class="muted mono">${result.savedOutputPath ?? "Not saved"}</div>
              </div>
              <div class="result-row">
                <strong>Counts</strong>
                <div class="muted">${result.successCount} succeeded / ${result.failedCount} failed</div>
              </div>
              <div class="result-row full">
                <strong>Markdown Preview</strong>
                <div class="markdown-preview mono">${escapeHtml(result.markdown)}</div>
              </div>
            </div>
            <div class="actions" style="margin-top: 16px;">
              <button id="result-open" class="ghost" ${result.savedOutputPath ? "" : "disabled"}>Open Markdown</button>
              <button id="result-reveal" class="ghost" ${result.savedOutputPath ? "" : "disabled"}>Reveal in Finder</button>
            </div>
          `
          : `<div class="result-row"><strong>No result yet</strong><div class="muted">Complete an import to populate this panel.</div></div>`
      }
    </section>
  `;
}

function render() {
  const snapshot = state.snapshot ?? {
    activeJob: null,
    progressLog: [],
    lastResult: null
  };
  const config = state.currentConfig;
  const validation = state.validation;
  const status = formatStatus(snapshot.activeJob?.status ?? "idle");
  const providerSummary = config
    ? `${config.provider} · ${config.model_id ?? config.modelId ?? config.model_id} · ${config.prompt_template ?? "General"}`
    : "Loading provider configuration...";

  root.innerHTML = `
    <main class="shell">
      <div class="frame">
        <section class="hero">
          <div class="pill-row">
            <h1>DuckDocs</h1>
            <p>Parse PDFs and Word files into linked markdown through the shared Rust engine.</p>
            <div class="status-line">
              <span class="pill">AI Engine · ${providerSummary}</span>
              <span class="status-tag ${status.tone}">${status.label}</span>
            </div>
          </div>
          <div class="cta-column">
            <div class="nav-row">
              ${panelButton("import", "Import")}
              ${panelButton("settings", "Settings")}
              ${panelButton("progress", "Progress")}
              ${panelButton("result", "Result")}
            </div>
            <p class="muted">No screen or accessibility permissions required. One window, one flow.</p>
          </div>
        </section>

        <section class="notice-stack" style="margin-top: 18px;">
          ${
            validation && !validation.ready
              ? `<div class="notice warn"><strong>Setup required.</strong><div class="muted">${validation.issues.map((issue) => issue.message).join(" ")}</div></div>`
              : `<div class="notice good"><strong>Provider ready.</strong><div class="muted">The engine config is valid for import-first parsing.</div></div>`
          }
          <div class="notice">
            <strong>Single window.</strong>
            <div class="muted">Settings, progress, and results now stay in the same desktop surface instead of opening detached windows.</div>
          </div>
        </section>

        ${renderPanel(snapshot)}
      </div>
    </main>
  `;

  bindEvents(snapshot);
}

function switchPanel(panel) {
  state.activePanel = panel;
  render();
}

function bindEvents(snapshot) {
  document.querySelectorAll("[data-panel]").forEach((element) => {
    element.addEventListener("click", () => switchPanel(element.dataset.panel));
  });

  document.querySelectorAll("#back-to-import").forEach((element) => {
    element.addEventListener("click", () => switchPanel("import"));
  });

  document.querySelector("#choose-file")?.addEventListener("click", async () => {
    const selected = await invoke("pick_import_file");
    if (selected) {
      state.selectedFile = selected;
      switchPanel("import");
    }
  });

  document.querySelector("#start-parse")?.addEventListener("click", async () => {
    if (!state.selectedFile) return;
    await invoke("start_parse", { request: { path: state.selectedFile.path, format: state.selectedFile.format } });
    state.activePanel = "progress";
    render();
  });

  document.querySelector("#cancel-parse")?.addEventListener("click", async () => {
    await invoke("cancel_parse");
  });

  document.querySelector("#go-settings")?.addEventListener("click", () => switchPanel("settings"));
  document.querySelector("#go-progress")?.addEventListener("click", () => switchPanel("progress"));
  document.querySelector("#go-result")?.addEventListener("click", () => switchPanel("result"));
  document.querySelector("#open-output")?.addEventListener("click", () => openSavedOutput(false));
  document.querySelector("#reveal-output")?.addEventListener("click", () => openSavedOutput(true));

  document.querySelector("#settings-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const payload = {
      ...state.currentConfig,
      provider: form.get("provider"),
      model_id: form.get("model_id"),
      api_key: form.get("api_key"),
      base_url: form.get("base_url") || null,
      prompt_template: form.get("prompt_template")
    };
    state.currentConfig = await invoke("save_engine_config", { payload });
    state.validation = await invoke("validate_engine_config", { payload: state.currentConfig });
    render();
  });

  document.querySelector("#validate-settings")?.addEventListener("click", async () => {
    state.validation = await invoke("validate_engine_config", { payload: state.currentConfig });
    render();
  });

  if (snapshot.lastResult) {
    document.querySelector("#result-open")?.addEventListener("click", () => openSavedOutput(false));
    document.querySelector("#result-reveal")?.addEventListener("click", () => openSavedOutput(true));
  }
}

async function openSavedOutput(reveal) {
  const path = state.snapshot?.lastResult?.savedOutputPath;
  if (!path) return;
  await invoke("open_saved_output", { path, reveal });
}

async function bootstrap() {
  state.snapshot = await invoke("app_snapshot");
  state.currentConfig = await invoke("load_engine_config");
  state.validation = await invoke("validate_engine_config");
  render();

  await event.listen("duckdocs://snapshot", (message) => {
    const payload = message.payload;
    state.snapshot = payload;
    if (payload.activeJob) {
      state.activePanel = "progress";
    } else if (payload.lastResult && state.activePanel === "progress") {
      state.activePanel = "result";
    }
    render();
  });
}

bootstrap().catch((error) => {
  root.innerHTML = `<main class="window-shell"><section class="card"><h1>DuckDocs failed to start</h1><p class="muted">${String(error)}</p></section></main>`;
});
