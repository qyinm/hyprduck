import { type ReactNode, useEffect, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  FileText,
  PanelLeftClose,
  PanelLeftOpen,
  Save,
  Settings,
} from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

type ActivePanel = "import" | "settings";

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
  requires_api_key: boolean;
  supports_base_url: boolean;
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
  listen<T>(eventName: string, handler: (message: TauriMessage<T>) => void | Promise<void>): Promise<TauriUnlisten>;
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

const NAV_ITEMS: { id: ActivePanel; label: string; icon: ReactNode }[] = [
  { id: "import", label: "Import", icon: <FileText aria-hidden="true" size={18} /> },
  { id: "settings", label: "Settings", icon: <Settings aria-hidden="true" size={18} /> },
];

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

function parseSummary(snapshot: UiSnapshot): string {
  const result = snapshot.lastResult;
  if (!result) {
    return "No completed imports yet.";
  }

  const counts = `${result.successCount} succeeded / ${result.failedCount} failed`;
  return result.savedOutputPath ? `${counts} · ${result.savedOutputPath}` : counts;
}

function sidebarButtonClass(active: boolean): string {
  return cn(
    "w-full justify-start gap-3 border border-transparent rounded-lg px-3 py-2 text-sm",
    active
      ? "bg-secondary text-foreground border-border"
      : "text-muted-foreground hover:text-foreground hover:bg-muted hover:border-border",
  );
}

function ImportPanel(props: {
  snapshot: UiSnapshot;
  selectedFile: FileSelection | null;
  onChooseFile: () => Promise<void>;
  onStartParse: () => Promise<void>;
  onCancelParse: () => Promise<void>;
  onOpenSavedOutput: (reveal: boolean) => Promise<void>;
}) {
  const { snapshot, selectedFile, onChooseFile, onStartParse, onCancelParse, onOpenSavedOutput } = props;
  const fileName = selectedFile?.path ? selectedFile.path.split("/").pop() : null;
  const canStart = Boolean(selectedFile) && !snapshot.activeJob;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Import dashboard</CardTitle>
          <CardDescription>Pick a file, start parsing, and review results in one workspace.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="rounded-lg border border-dashed border-border bg-muted/20 p-6">
            <p className="font-medium">{fileName ?? "No file selected"}</p>
            <p className="text-sm text-muted-foreground">
              {selectedFile?.format.toUpperCase() ?? "PDF, DOCX, DOC supported"}
            </p>
          </div>

          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void onChooseFile()} type="button">
              Choose file
            </Button>
            <Button onClick={() => void onStartParse()} disabled={!canStart} type="button">
              Start import
            </Button>
            <Button variant="outline" disabled={!snapshot.activeJob} onClick={() => void onCancelParse()} type="button">
              Cancel
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Latest output</CardTitle>
          <CardDescription>Review results from your last completed import.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <p className="text-sm text-muted-foreground">{parseSummary(snapshot)}</p>
          <div className="flex gap-2">
            <Button
              variant="ghost"
              disabled={!snapshot.lastResult?.savedOutputPath}
              onClick={() => void onOpenSavedOutput(false)}
              type="button"
            >
              Open markdown
            </Button>
            <Button
              variant="ghost"
              disabled={!snapshot.lastResult?.savedOutputPath}
              onClick={() => void onOpenSavedOutput(true)}
              type="button"
            >
              Reveal in Finder
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Markdown preview</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="mb-2 text-sm text-muted-foreground">
            {snapshot.lastResult ? "Latest generated output from the Rust engine." : "No result yet."}
          </p>
          <pre className="max-h-72 overflow-auto rounded-lg border bg-muted/20 p-3 text-xs font-mono leading-relaxed">
            {snapshot.lastResult?.markdown ?? "No markdown generated yet."}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}

type SettingsTab = "general" | "ai";

interface ProviderState {
  apiKey: string;
  expanded: boolean;
}

function SettingsPanel(props: {
  config: EngineConfigPayload | null;
  validation: ValidateProviderResponseData | null;
  onSave: (payload: EngineConfigPayload) => Promise<void>;
  onValidate: (payload: EngineConfigPayload | null) => Promise<void>;
}) {
  const { config, validation, onSave, onValidate } = props;
  const [tab, setTab] = useState<SettingsTab>("ai");
  const [promptTemplate, setPromptTemplate] = useState("General");
  const [selectedModel, setSelectedModel] = useState("");
  const [activeProvider, setActiveProvider] = useState("open_router");
  const [providerStates, setProviderStates] = useState<Map<string, ProviderState>>(new Map());

  useEffect(() => {
    if (config) {
      setActiveProvider(config.provider);
      setSelectedModel(config.model_id);
      setPromptTemplate(config.prompt_template ?? "General");
      setProviderStates((prev) => {
        const next = new Map(prev);
        for (const opt of config.provider_options) {
          next.set(opt.id, { apiKey: config.api_key, expanded: false });
        }
        return next;
      });
    }
  }, [config]);

  const updateApiKey = (providerId: string, key: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? { apiKey: "", expanded: false };
      next.set(providerId, { ...existing, apiKey: key });
      return next;
    });
  };

  const toggleExpanded = (providerId: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? { apiKey: "", expanded: false };
      next.set(providerId, { ...existing, expanded: !existing.expanded });
      return next;
    });
  };

  const handleProviderChange = async (providerId: string) => {
    setActiveProvider(providerId);
    const models = await invoke<string[]>("get_models_for_provider", { providerSlug: providerId });
    if (models.length > 0) {
      setSelectedModel(models[0]);
    }
  };

  const [availableModels, setAvailableModels] = useState<string[]>([]);

  useEffect(() => {
    if (activeProvider) {
      invoke<string[]>("get_models_for_provider", { providerSlug: activeProvider })
        .then((models) => setAvailableModels(models))
        .catch(() => setAvailableModels([]));
    }
  }, [activeProvider]);

  const activeApiKey = providerStates.get(activeProvider)?.apiKey ?? "";

  const handleSave = async () => {
    const payload: EngineConfigPayload = {
      provider: activeProvider,
      model_id: selectedModel,
      api_key: activeApiKey,
      base_url: null,
      prompt_template: promptTemplate,
      provider_options: config?.provider_options ?? [],
      model_options: availableModels,
      prompt_template_options: config?.prompt_template_options ?? [],
    };
    await onSave(payload);
  };

  if (!config) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Settings</CardTitle>
          <CardDescription>Loading engine configuration...</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      {/* Tabs */}
      <div className="flex gap-1 rounded-lg bg-muted p-1">
        <button
          type="button"
          onClick={() => setTab("general")}
          className={`flex-1 rounded-md px-3 py-2 text-sm font-medium transition ${
            tab === "general"
              ? "bg-card text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground"
          }`}
        >
          General
        </button>
        <button
          type="button"
          onClick={() => setTab("ai")}
          className={`flex-1 rounded-md px-3 py-2 text-sm font-medium transition ${
            tab === "ai"
              ? "bg-card text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground"
          }`}
        >
          AI
        </button>
      </div>

      {tab === "general" && (
        <Card>
          <CardHeader>
            <CardTitle>General settings</CardTitle>
            <CardDescription>Configure prompt templates and output behavior.</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="prompt-template-select">Prompt template</Label>
              <select
                id="prompt-template-select"
                className="h-9 w-full rounded-md border border-input bg-background px-3"
                value={promptTemplate}
                onChange={(e) => setPromptTemplate(e.target.value)}
              >
                {(config.prompt_template_options ?? ["General"]).map((option) => (
                  <option key={option} value={option}>{option}</option>
                ))}
              </select>
            </div>
          </CardContent>
        </Card>
      )}

      {tab === "ai" && (
        <div className="space-y-4">
          {/* Active Provider & Model Selector */}
          <Card>
            <CardHeader>
              <CardTitle>Active AI provider</CardTitle>
              <CardDescription>Select the provider and model for document parsing.</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="active-provider">Provider</Label>
                  <select
                    id="active-provider"
                    className="h-9 w-full rounded-md border border-input bg-background px-3"
                    value={activeProvider}
                    onChange={(e) => void handleProviderChange(e.target.value)}
                  >
                    {config.provider_options.map((opt) => (
                      <option key={opt.id} value={opt.id}>{opt.label}</option>
                    ))}
                  </select>
                </div>
                <div className="space-y-2">
                  <Label htmlFor="active-model">Model</Label>
                  <select
                    id="active-model"
                    className="h-9 w-full rounded-md border border-input bg-background px-3"
                    value={selectedModel}
                    onChange={(e) => setSelectedModel(e.target.value)}
                  >
                    {availableModels.map((m) => (
                      <option key={m} value={m}>{m}</option>
                    ))}
                  </select>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Provider List */}
          {config.provider_options.map((opt) => {
            const state = providerStates.get(opt.id) ?? { apiKey: "", expanded: false };
            return (
              <Card key={opt.id}>
                <div
                  className="flex cursor-pointer items-center justify-between p-4"
                  onClick={() => toggleExpanded(opt.id)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") toggleExpanded(opt.id); }}
                >
                  <div className="flex items-center gap-3">
                    <span className="text-sm font-medium">{opt.label}</span>
                    {activeProvider === opt.id && (
                      <span className="rounded-full bg-emerald-100 px-2 py-0.5 text-xs font-medium text-emerald-700">
                        Active
                      </span>
                    )}
                  </div>
                  {state.expanded ? (
                    <ChevronDown size={16} className="text-muted-foreground" />
                  ) : (
                    <ChevronRight size={16} className="text-muted-foreground" />
                  )}
                </div>
                {state.expanded && (
                  <div className="border-t px-4 py-3 space-y-3">
                    <div className="space-y-2">
                      <Label>API Key</Label>
                      <div className="flex gap-2">
                        <Input
                          autoComplete="off"
                          onChange={(e) => updateApiKey(opt.id, e.target.value)}
                          placeholder={opt.requires_api_key ? "Required" : "Optional"}
                          type="password"
                          value={state.apiKey}
                        />
                      </div>
                    </div>
                  </div>
                )}
              </Card>
            );
          })}

          {/* Bottom actions */}
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void handleSave()} type="button">
              <Save size={16} className="mr-2" />
              Save Settings
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                const payload: EngineConfigPayload | null = activeProvider
                  ? {
                      provider: activeProvider,
                      model_id: selectedModel,
                      api_key: activeApiKey,
                      base_url: null,
                      prompt_template: promptTemplate,
                      provider_options: config.provider_options,
                      model_options: availableModels,
                      prompt_template_options: config.prompt_template_options,
                    }
                  : null;
                void onValidate(payload);
              }}
              type="button"
            >
              Validate
            </Button>
          </div>

          {/* Validation result */}
          {validation && (
            <Card>
              <CardContent className="pt-4">
                <div
                  className={`rounded-lg border p-3 ${
                    validation.ready
                      ? "border-emerald-300/60 bg-emerald-50"
                      : "border-amber-300/60 bg-amber-50"
                  }`}
                >
                  <p className="font-medium">{validation.ready ? "Validation passed" : "Needs attention"}</p>
                  <p className="text-sm text-muted-foreground">
                    {validation.issues.map((i) => i.message).join(" ") || "No issues detected."}
                  </p>
                </div>
              </CardContent>
            </Card>
          )}
        </div>
      )}
    </div>
  );
}

export function App() {
  const [snapshot, setSnapshot] = useState<UiSnapshot>(EMPTY_SNAPSHOT);
  const [currentConfig, setCurrentConfig] = useState<EngineConfigPayload | null>(null);
  const [validation, setValidation] = useState<ValidateProviderResponseData | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileSelection | null>(null);
  const [activePanel, setActivePanel] = useState<ActivePanel>("import");
  const [sidebarOpen, setSidebarOpen] = useState(true);
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
        setSnapshot(message.payload);
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
    setActivePanel("import");
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

  if (startupError) {
    return (
      <main className="grid min-h-screen place-items-center bg-background p-6">
        <Card className="max-w-xl">
          <CardHeader>
            <CardTitle>DuckDocs failed to start</CardTitle>
            <CardDescription>Required runtime is missing.</CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">{startupError}</p>
          </CardContent>
        </Card>
      </main>
    );
  }

  return (
    <main className="h-screen w-screen overflow-hidden bg-background text-foreground">
      <div className="h-full w-full overflow-hidden bg-card">
        <div className="grid h-full grid-cols-1 overflow-hidden sm:grid-cols-[auto_minmax(0,1fr)]">
          <aside
            className={cn(
              "relative flex min-h-0 flex-col border-r border-sidebar-border bg-sidebar",
              "transition-[width] duration-200 ease-in-out",
              sidebarOpen ? "w-72" : "w-16",
            )}
          >
            <div
              className={cn(
                "px-3 pt-3",
                sidebarOpen ? "flex items-center justify-between" : "flex flex-col items-center gap-2",
              )}
            >
              <div className={cn("space-y-1 px-1", sidebarOpen ? "block" : "hidden")}>
                <p className="text-xs text-muted-foreground">DuckDocs</p>
                <p className="text-lg font-semibold">Parsing Dashboard</p>
              </div>

              <div className={cn("px-1", sidebarOpen ? "hidden" : "block")}>
                <p className="text-xs font-semibold text-muted-foreground">DD</p>
              </div>

              <Button
                aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
                className="rounded-full"
                onClick={() => setSidebarOpen((current) => !current)}
                size="icon"
                variant="outline"
                type="button"
              >
                {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
              </Button>
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-5 p-3 overflow-y-auto">
              <nav className={cn("space-y-1", sidebarOpen ? "" : "pt-2")}>
                {NAV_ITEMS.map((item) => (
                  <Button
                    key={item.id}
                    aria-current={activePanel === item.id ? "page" : undefined}
                    className={cn(sidebarButtonClass(activePanel === item.id), sidebarOpen ? "" : "justify-center px-0")}
                    onClick={() => setActivePanel(item.id)}
                    size="sm"
                    variant={activePanel === item.id ? "default" : "ghost"}
                    type="button"
                  >
                    <span aria-hidden="true">{item.icon}</span>
                    {sidebarOpen ? <span className="font-medium">{item.label}</span> : null}
                  </Button>
                ))}
              </nav>
            </div>
          </aside>

          <section className="flex min-w-0 flex-col overflow-hidden">
            <header className="sticky top-0 z-10 flex h-16 flex-none items-center border-b bg-card/80 px-5 backdrop-blur">
              <div>
                <p className="text-xs uppercase tracking-wide text-muted-foreground">Workspace</p>
                <h1 className="text-xl font-semibold">Document parsing</h1>
              </div>
            </header>

            <div className="min-h-0 flex-1 overflow-y-auto p-5">
              <section className="space-y-4">
                {activePanel === "settings" ? (
                  <SettingsPanel
                    config={currentConfig}
                    onSave={saveConfig}
                    onValidate={validateConfig}
                    validation={validation}
                  />
                ) : (
                  <ImportPanel
                    onCancelParse={cancelParse}
                    onChooseFile={chooseFile}
                    onOpenSavedOutput={openSavedOutput}
                    onStartParse={startParse}
                    selectedFile={selectedFile}
                    snapshot={snapshot}
                  />
                )}
              </section>
            </div>
          </section>
        </div>
      </div>
    </main>
  );
}
