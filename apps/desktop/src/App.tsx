import { useEffect, useState } from "react";

type ActivePanel = "import" | "settings" | "progress" | "result";
type StatusTone = "" | "good" | "warn" | "bad";

interface UiSnapshot {
  activeJob: ActiveJobSnapshot | null;
  progressLog: ProgressEntry[];
  lastResult: CompletedResultSnapshot | null;
}

interface ActiveJobSnapshot {
  jobId: string;
  filePath: string;
  format: string;
  status: string;
  progressPercent: number;
  lastMessage: string | null;
}

interface ProgressEntry {
  phase: string;
  message: string;
  timestamp: string;
}

interface CompletedResultSnapshot {
  savedOutputPath: string | null;
  successCount: number;
  failedCount: number;
  markdown: string;
}

interface FileSelection {
  path: string;
  format: string;
}

interface ProviderOption {
  id: string;
  label: string;
}

interface ValidationIssue {
  message: string;
}

interface EngineConfigPayload {
  provider: string;
  model_id: string;
  api_key: string;
  base_url: string | null;
  prompt_template: string;
  provider_options: ProviderOption[];
  model_options: string[];
  prompt_template_options: string[];
}

interface ValidateProviderResponseData {
  ready: boolean;
  issues: ValidationIssue[];
}

interface TauriMessage<T> {
  payload: T;
}

type TauriUnlisten = () => void;

interface TauriEventApi {
  listen<T>(
    eventName: string,
    handler: (message: TauriMessage<T>) => void | Promise<void>,
  ): Promise<TauriUnlisten>;
}

interface TauriCoreApi {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

interface TauriGlobalApi {
  core: TauriCoreApi;
  event: TauriEventApi;
}

declare global {
  interface Window {
    __TAURI__?: TauriGlobalApi;
  }
}

const EMPTY_SNAPSHOT: UiSnapshot = {
  activeJob: null,
  progressLog: [],
  lastResult: null,
};

function getTauri(): TauriGlobalApi {
  const tauri = window.__TAURI__;
  if (!tauri) {
    throw new Error("DuckDocs desktop UI requires Tauri global APIs.");
  }
  return tauri;
}

async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return getTauri().core.invoke<T>(command, args);
}

function formatStatus(kind: string): { label: string; tone: StatusTone } {
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

function parseSummary(snapshot: UiSnapshot): string {
  const result = snapshot.lastResult;
  if (!result) {
    return "No completed imports yet.";
  }

  const counts = `${result.successCount} succeeded / ${result.failedCount} failed`;
  return result.savedOutputPath ? `${counts} · ${result.savedOutputPath}` : counts;
}

function PanelButton(props: {
  id: ActivePanel;
  label: string;
  activePanel: ActivePanel;
  onSelect: (panel: ActivePanel) => void;
}) {
  const { id, label, activePanel, onSelect } = props;
  const activeClass = activePanel === id ? "active" : "";
  return (
    <button className={`ghost nav-pill ${activeClass}`.trim()} onClick={() => onSelect(id)} type="button">
      {label}
    </button>
  );
}

function ImportPanel(props: {
  snapshot: UiSnapshot;
  selectedFile: FileSelection | null;
  onChooseFile: () => Promise<void>;
  onStartParse: () => Promise<void>;
  onCancelParse: () => Promise<void>;
  onPanelChange: (panel: ActivePanel) => void;
  onOpenSavedOutput: (reveal: boolean) => Promise<void>;
}) {
  const { snapshot, selectedFile, onChooseFile, onStartParse, onCancelParse, onPanelChange, onOpenSavedOutput } =
    props;
  const status = formatStatus(snapshot.activeJob?.status ?? "idle");
  const canStart = Boolean(selectedFile) && !snapshot.activeJob;

  return (
    <section className="dashboard">
      <article className="card">
        <h2>Import Dashboard</h2>
        <p className="muted">
          Choose a file, start parsing, and stay in one window while the job moves from config to progress to
          results.
        </p>
        <div className="file-drop">
          <strong>{selectedFile?.path ?? "No file selected"}</strong>
          <div className="muted">{selectedFile?.format.toUpperCase() ?? "PDF, DOCX, and DOC are supported."}</div>
        </div>
        <div className="actions" style={{ marginTop: "16px" }}>
          <button onClick={() => void onChooseFile()} type="button">
            Choose File
          </button>
          <button disabled={!canStart} onClick={() => void onStartParse()} type="button">
            Start Import
          </button>
          <button className="secondary" onClick={() => onPanelChange("settings")} type="button">
            Settings
          </button>
          <button className="secondary" onClick={() => onPanelChange("progress")} type="button">
            Progress
          </button>
          <button className="secondary" onClick={() => onPanelChange("result")} type="button">
            Result
          </button>
        </div>

        {snapshot.activeJob ? (
          <div className="card inset-card">
            <h3>Current Job</h3>
            <div className="muted mono">{snapshot.activeJob.filePath}</div>
            <div className="progress-bar" style={{ marginTop: "12px" }}>
              <span style={{ width: `${Math.max(6, snapshot.activeJob.progressPercent)}%` }} />
            </div>
            <div className="status-line" style={{ marginTop: "12px" }}>
              <span className={`status-tag ${status.tone}`.trim()}>{status.label}</span>
              <span className="muted">{snapshot.activeJob.lastMessage ?? "Waiting for engine events..."}</span>
            </div>
            <div className="actions" style={{ marginTop: "12px" }}>
              <button className="ghost" onClick={() => void onCancelParse()} type="button">
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <div className="card inset-card">
            <h3>Latest Output</h3>
            <div className="muted">{parseSummary(snapshot)}</div>
            <div className="actions" style={{ marginTop: "12px" }}>
              <button
                className="ghost"
                disabled={!snapshot.lastResult?.savedOutputPath}
                onClick={() => void onOpenSavedOutput(false)}
                type="button"
              >
                Open Markdown
              </button>
              <button
                className="ghost"
                disabled={!snapshot.lastResult?.savedOutputPath}
                onClick={() => void onOpenSavedOutput(true)}
                type="button"
              >
                Reveal in Finder
              </button>
            </div>
          </div>
        )}
      </article>

      <aside className="card">
        <h2>Current State</h2>
        <div className="event-log">
          {snapshot.progressLog.length > 0 ? (
            snapshot.progressLog.slice(0, 8).map((entry) => (
              <div className="event-row" key={`${entry.phase}-${entry.timestamp}-${entry.message}`}>
                <strong>{entry.phase}</strong>
                <div className="muted">{entry.message}</div>
              </div>
            ))
          ) : (
            <div className="event-row">
              <strong>No events yet</strong>
              <div className="muted">Start an import to populate progress events.</div>
            </div>
          )}
        </div>
      </aside>
    </section>
  );
}

function SettingsPanel(props: {
  config: EngineConfigPayload | null;
  validation: ValidateProviderResponseData | null;
  onBack: () => void;
  onSave: (payload: EngineConfigPayload) => Promise<void>;
  onValidate: (payload: EngineConfigPayload | null) => Promise<void>;
}) {
  const { config, validation, onBack, onSave, onValidate } = props;
  const [formState, setFormState] = useState<EngineConfigPayload | null>(config);

  useEffect(() => {
    setFormState(config);
  }, [config]);

  if (!formState) {
    return (
      <section className="card panel-card">
        <div className="panel-heading">
          <div>
            <h2>AI Settings</h2>
            <p className="muted">Loading engine configuration...</p>
          </div>
          <button className="ghost" onClick={onBack} type="button">
            Back to Import
          </button>
        </div>
      </section>
    );
  }

  const updateField = <K extends keyof EngineConfigPayload>(key: K, value: EngineConfigPayload[K]) => {
    setFormState((current) => (current ? { ...current, [key]: value } : current));
  };

  return (
    <section className="card panel-card">
      <div className="panel-heading">
        <div>
          <h2>AI Settings</h2>
          <p className="muted">Provider, model, API key, Ollama endpoint, and parsing template stay Rust-owned.</p>
        </div>
        <button className="ghost" onClick={onBack} type="button">
          Back to Import
        </button>
      </div>

      <form
        className="field-grid"
        onSubmit={(event) => {
          event.preventDefault();
          void onSave(formState);
        }}
      >
        <label className="full">
          <span className="muted">Provider</span>
          <select onChange={(event) => updateField("provider", event.target.value)} value={formState.provider}>
            {formState.provider_options.map((option) => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="muted">Model</span>
          <select onChange={(event) => updateField("model_id", event.target.value)} value={formState.model_id}>
            {formState.model_options.map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="muted">Prompt Template</span>
          <select
            onChange={(event) => updateField("prompt_template", event.target.value)}
            value={formState.prompt_template}
          >
            {formState.prompt_template_options.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>
        <label className="full">
          <span className="muted">API Key</span>
          <input
            autoComplete="off"
            onChange={(event) => updateField("api_key", event.target.value)}
            type="password"
            value={formState.api_key}
          />
        </label>
        <label className="full">
          <span className="muted">Base URL</span>
          <input
            onChange={(event) => updateField("base_url", event.target.value || null)}
            placeholder="Optional custom endpoint"
            type="text"
            value={formState.base_url ?? ""}
          />
        </label>
        <div className="full actions">
          <button type="submit">Save Settings</button>
          <button className="secondary" onClick={() => void onValidate(formState)} type="button">
            Validate
          </button>
        </div>
      </form>

      <div className="notice-stack" style={{ marginTop: "18px" }}>
        {validation ? (
          <div className={`notice ${validation.ready ? "good" : "warn"}`.trim()}>
            <strong>{validation.ready ? "Ready to import" : "Needs attention"}</strong>
            <div className="muted">{validation.issues.map((issue) => issue.message).join(" ") || "Validation passed."}</div>
          </div>
        ) : (
          <div className="notice">
            <strong>Validation pending.</strong>
            <div className="muted">Save or validate the current engine configuration.</div>
          </div>
        )}
      </div>
    </section>
  );
}

function ProgressPanel(props: { snapshot: UiSnapshot; onBack: () => void }) {
  const { snapshot, onBack } = props;

  return (
    <section className="card panel-card">
      <div className="panel-heading">
        <div>
          <h2>Progress</h2>
          <p className="muted">Parse events stream from the Rust engine stderr channel into the shared desktop store.</p>
        </div>
        <button className="ghost" onClick={onBack} type="button">
          Back to Import
        </button>
      </div>
      <div className="event-log">
        {snapshot.progressLog.length > 0 ? (
          snapshot.progressLog.map((entry) => (
            <div className="event-row" key={`${entry.phase}-${entry.timestamp}-${entry.message}`}>
              <strong>{entry.phase}</strong>
              <div className="muted">{entry.message}</div>
              <div className="muted mono">{entry.timestamp}</div>
            </div>
          ))
        ) : (
          <div className="event-row">
            <strong>No active parse</strong>
            <div className="muted">Run an import from the main view to inspect progress here.</div>
          </div>
        )}
      </div>
    </section>
  );
}

function ResultPanel(props: {
  snapshot: UiSnapshot;
  onBack: () => void;
  onOpenSavedOutput: (reveal: boolean) => Promise<void>;
}) {
  const { snapshot, onBack, onOpenSavedOutput } = props;
  const result = snapshot.lastResult;

  return (
    <section className="card panel-card">
      <div className="panel-heading">
        <div>
          <h2>Result</h2>
          <p className="muted">Saved markdown package metadata and a preview of the generated output.</p>
        </div>
        <button className="ghost" onClick={onBack} type="button">
          Back to Import
        </button>
      </div>
      {result ? (
        <>
          <div className="result-grid">
            <div className="result-row">
              <strong>Saved Output</strong>
              <div className="muted mono">{result.savedOutputPath ?? "Not saved"}</div>
            </div>
            <div className="result-row">
              <strong>Counts</strong>
              <div className="muted">
                {result.successCount} succeeded / {result.failedCount} failed
              </div>
            </div>
            <div className="result-row full">
              <strong>Markdown Preview</strong>
              <div className="markdown-preview mono">{result.markdown}</div>
            </div>
          </div>
          <div className="actions" style={{ marginTop: "16px" }}>
            <button
              className="ghost"
              disabled={!result.savedOutputPath}
              onClick={() => void onOpenSavedOutput(false)}
              type="button"
            >
              Open Markdown
            </button>
            <button
              className="ghost"
              disabled={!result.savedOutputPath}
              onClick={() => void onOpenSavedOutput(true)}
              type="button"
            >
              Reveal in Finder
            </button>
          </div>
        </>
      ) : (
        <div className="result-row">
          <strong>No result yet</strong>
          <div className="muted">Complete an import to populate this panel.</div>
        </div>
      )}
    </section>
  );
}

export function App() {
  const [snapshot, setSnapshot] = useState<UiSnapshot>(EMPTY_SNAPSHOT);
  const [currentConfig, setCurrentConfig] = useState<EngineConfigPayload | null>(null);
  const [validation, setValidation] = useState<ValidateProviderResponseData | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileSelection | null>(null);
  const [activePanel, setActivePanel] = useState<ActivePanel>("import");
  const [startupError, setStartupError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: TauriUnlisten | null = null;

    const bootstrap = async () => {
      const tauri = getTauri();
      const [initialSnapshot, initialConfig, initialValidation] = await Promise.all([
        invoke<UiSnapshot>("app_snapshot"),
        invoke<EngineConfigPayload>("load_engine_config"),
        invoke<ValidateProviderResponseData>("validate_engine_config"),
      ]);
      setSnapshot(initialSnapshot);
      setCurrentConfig(initialConfig);
      setValidation(initialValidation);

      unlisten = await tauri.event.listen<UiSnapshot>("duckdocs://snapshot", (message) => {
        const payload = message.payload;
        setSnapshot(payload);
        setActivePanel((current) => {
          if (payload.activeJob) {
            return "progress";
          }
          if (payload.lastResult && current === "progress") {
            return "result";
          }
          return current;
        });
      });
    };

    void bootstrap().catch((error: unknown) => {
      setStartupError(String(error));
    });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  const status = formatStatus(snapshot.activeJob?.status ?? "idle");
  const providerSummary = currentConfig
    ? `${currentConfig.provider} · ${currentConfig.model_id} · ${currentConfig.prompt_template ?? "General"}`
    : "Loading provider configuration...";

  const chooseFile = async () => {
    const selection = await invoke<FileSelection | null>("pick_import_file");
    if (selection) {
      setSelectedFile(selection);
      setActivePanel("import");
    }
  };

  const startParse = async () => {
    if (!selectedFile) {
      return;
    }
    await invoke<void>("start_parse", {
      request: {
        path: selectedFile.path,
        format: selectedFile.format,
      },
    });
    setActivePanel("progress");
  };

  const cancelParse = async () => {
    await invoke<void>("cancel_parse");
  };

  const openSavedOutput = async (reveal: boolean) => {
    const path = snapshot.lastResult?.savedOutputPath;
    if (!path) {
      return;
    }
    await invoke<void>("open_saved_output", { path, reveal });
  };

  const saveConfig = async (payload: EngineConfigPayload) => {
    const saved = await invoke<EngineConfigPayload>("save_engine_config", { payload });
    const nextValidation = await invoke<ValidateProviderResponseData>("validate_engine_config", { payload: saved });
    setCurrentConfig(saved);
    setValidation(nextValidation);
  };

  const validateConfig = async (payload: EngineConfigPayload | null) => {
    const nextValidation = await invoke<ValidateProviderResponseData>("validate_engine_config", { payload });
    setValidation(nextValidation);
  };

  let panel: React.ReactNode;
  switch (activePanel) {
    case "settings":
      panel = (
        <SettingsPanel
          config={currentConfig}
          onBack={() => setActivePanel("import")}
          onSave={saveConfig}
          onValidate={validateConfig}
          validation={validation}
        />
      );
      break;
    case "progress":
      panel = <ProgressPanel onBack={() => setActivePanel("import")} snapshot={snapshot} />;
      break;
    case "result":
      panel = (
        <ResultPanel onBack={() => setActivePanel("import")} onOpenSavedOutput={openSavedOutput} snapshot={snapshot} />
      );
      break;
    default:
      panel = (
        <ImportPanel
          onCancelParse={cancelParse}
          onChooseFile={chooseFile}
          onOpenSavedOutput={openSavedOutput}
          onPanelChange={setActivePanel}
          onStartParse={startParse}
          selectedFile={selectedFile}
          snapshot={snapshot}
        />
      );
      break;
  }

  if (startupError) {
    return (
      <main className="window-shell">
        <section className="card">
          <h1>DuckDocs failed to start</h1>
          <p className="muted">{startupError}</p>
        </section>
      </main>
    );
  }

  return (
    <main className="shell">
      <div className="frame">
        <section className="hero">
          <div className="pill-row">
            <h1>DuckDocs</h1>
            <p>Parse PDFs and Word files into linked markdown through the shared Rust engine.</p>
            <div className="status-line">
              <span className="pill">AI Engine · {providerSummary}</span>
              <span className={`status-tag ${status.tone}`.trim()}>{status.label}</span>
            </div>
          </div>
          <div className="cta-column">
            <div className="nav-row">
              <PanelButton activePanel={activePanel} id="import" label="Import" onSelect={setActivePanel} />
              <PanelButton activePanel={activePanel} id="settings" label="Settings" onSelect={setActivePanel} />
              <PanelButton activePanel={activePanel} id="progress" label="Progress" onSelect={setActivePanel} />
              <PanelButton activePanel={activePanel} id="result" label="Result" onSelect={setActivePanel} />
            </div>
            <p className="muted">No screen or accessibility permissions required. One window, one flow.</p>
          </div>
        </section>

        <section className="notice-stack" style={{ marginTop: "18px" }}>
          {validation && !validation.ready ? (
            <div className="notice warn">
              <strong>Setup required.</strong>
              <div className="muted">{validation.issues.map((issue) => issue.message).join(" ")}</div>
            </div>
          ) : (
            <div className="notice good">
              <strong>Provider ready.</strong>
              <div className="muted">The engine config is valid for import-first parsing.</div>
            </div>
          )}
          <div className="notice">
            <strong>Single window.</strong>
            <div className="muted">
              Settings, progress, and results now stay in the same desktop surface instead of opening detached
              windows.
            </div>
          </div>
        </section>

        {panel}
      </div>
    </main>
  );
}
