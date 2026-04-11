import { type ReactNode, useEffect, useReducer, useState } from "react";
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  FileText,
  PanelLeftClose,
  PanelLeftOpen,
  Save,
  Settings,
  Share2,
  Sparkles,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { GraphWorkspace } from "@/features/workspace/GraphWorkspace";
import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import {
  createInitialWorkspaceUiState,
  workspaceUiStateReducer,
} from "@/features/workspace/state";
import type {
  WorkspaceApplyCorrectionRequest,
  WorkspaceAnswerProjectRequest,
  WorkspaceProject,
} from "@/features/workspace/types";
import { cn } from "@/lib/utils";

type ActivePanel = "import" | "graph";
type SettingsTab = "general" | "ai";

interface UiSnapshot {
  activeJob: ActiveJobSnapshot | null;
  progressLog: ProgressEntry[];
  lastResult: CompletedResultSnapshot | null;
  lastProjectId?: string | null;
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

const MAIN_NAV_ITEMS: { id: ActivePanel; label: string; icon: ReactNode }[] = [
  {
    id: "import",
    label: "Import",
    icon: <FileText aria-hidden="true" size={18} />,
  },
  {
    id: "graph",
    label: "Graph",
    icon: <Share2 aria-hidden="true" size={18} />,
  },
];

const SETTINGS_NAV_ITEMS: {
  id: SettingsTab;
  label: string;
  icon: ReactNode;
}[] = [
  {
    id: "general",
    label: "General",
    icon: <Settings aria-hidden="true" size={18} />,
  },
  { id: "ai", label: "AI", icon: <Sparkles aria-hidden="true" size={18} /> },
];

const EMPTY_SNAPSHOT: UiSnapshot = {
  activeJob: null,
  progressLog: [],
  lastResult: null,
  lastProjectId: null,
};

function getTauri(): TauriGlobalApi {
  const tauri = window.__TAURI__;
  if (!tauri) {
    throw new Error("DuckDocs desktop UI requires Tauri global APIs.");
  }
  return tauri;
}

async function invoke<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return getTauri().core.invoke<T>(command, args);
}

function parseSummary(snapshot: UiSnapshot): string {
  const result = snapshot.lastResult;
  if (!result) {
    return "No completed imports yet.";
  }

  const counts = `${result.successCount} succeeded / ${result.failedCount} failed`;
  return result.savedOutputPath
    ? `${counts} · ${result.savedOutputPath}`
    : counts;
}

function sidebarButtonClass(active: boolean): string {
  return cn(
    "w-full justify-start gap-3 border border-transparent rounded-lg px-3 py-2 text-sm",
    active
      ? "bg-secondary text-foreground border-border"
      : "text-muted-foreground hover:text-foreground hover:bg-gray-400/15",
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
  const {
    snapshot,
    selectedFile,
    onChooseFile,
    onStartParse,
    onCancelParse,
    onOpenSavedOutput,
  } = props;
  const fileName = selectedFile?.path
    ? selectedFile.path.split("/").pop()
    : null;
  const canStart = Boolean(selectedFile) && !snapshot.activeJob;

  return (
    <div className="space-y-8">
      {/* Import section */}
      <section>
        <h2 className="text-base font-semibold mb-1">Import</h2>
        <p className="text-sm text-muted-foreground mb-3">
          Pick a file and start parsing.
        </p>

        <div className="rounded-lg border border-dashed border-border bg-muted/10 p-6 mb-3">
          <p className="font-medium">{fileName ?? "No file selected"}</p>
          <p className="text-sm text-muted-foreground">
            {selectedFile?.format.toUpperCase() ?? "PDF, DOCX, DOC supported"}
          </p>
        </div>

        <div className="flex flex-wrap gap-2">
          <Button onClick={() => void onChooseFile()} type="button">
            Choose file
          </Button>
          <Button
            onClick={() => void onStartParse()}
            disabled={!canStart}
            type="button"
          >
            Start import
          </Button>
          <Button
            variant="outline"
            disabled={!snapshot.activeJob}
            onClick={() => void onCancelParse()}
            type="button"
          >
            Cancel
          </Button>
        </div>
      </section>

      {/* Results section */}
      <section>
        <h2 className="text-base font-semibold mb-1">Latest output</h2>
        <p className="text-sm text-muted-foreground mb-3">
          Review results from your last completed import.
        </p>

        <p className="text-sm text-muted-foreground mb-2">
          {parseSummary(snapshot)}
        </p>
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
      </section>

      {/* Markdown preview section */}
      <section>
        <h2 className="text-base font-semibold mb-3">Markdown preview</h2>
        <pre className="max-h-72 overflow-auto rounded-lg border bg-muted/10 p-3 text-xs font-mono leading-relaxed">
          {snapshot.lastResult?.markdown ?? "No markdown generated yet."}
        </pre>
      </section>
    </div>
  );
}

interface ProviderState {
  apiKey: string;
  baseUrl: string;
  expanded: boolean;
  showAdvanced: boolean;
}

function SettingsPanel(props: {
  config: EngineConfigPayload | null;
  validation: ValidateProviderResponseData | null;
  onSave: (payload: EngineConfigPayload) => Promise<void>;
  onValidate: (payload: EngineConfigPayload | null) => Promise<void>;
  tab: SettingsTab;
  onTabChange: (tab: SettingsTab) => void;
}) {
  const {
    config,
    validation,
    onSave,
    onValidate,
    tab,
    onTabChange: setTab,
  } = props;
  const [promptTemplate, setPromptTemplate] = useState("General");
  const [selectedModel, setSelectedModel] = useState("");
  const [activeProvider, setActiveProvider] = useState("open_router");
  const [providerStates, setProviderStates] = useState<
    Map<string, ProviderState>
  >(new Map());

  useEffect(() => {
    if (config) {
      setActiveProvider(config.provider);
      setSelectedModel(config.model_id);
      setPromptTemplate(config.prompt_template ?? "General");
      setProviderStates((prev) => {
        const next = new Map(prev);
        for (const opt of config.provider_options) {
          next.set(opt.id, {
            apiKey: config.api_key,
            baseUrl: "",
            expanded: false,
          showAdvanced: false,
          });
        }
        return next;
      });
    }
  }, [config]);

  const updateApiKey = (providerId: string, key: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? {
        apiKey: "",
        baseUrl: "",
        expanded: false,
        showAdvanced: false,
      };
      next.set(providerId, { ...existing, apiKey: key });
      return next;
    });
  };

  const toggleExpanded = (providerId: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? {
        apiKey: "",
        baseUrl: "",
        expanded: false,
        showAdvanced: false,
      };
      next.set(providerId, { ...existing, expanded: !existing.expanded });
      return next;
    });
  };

  const updateBaseUrl = (providerId: string, url: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? {
        apiKey: "",
        baseUrl: "",
        expanded: false,
        showAdvanced: false,
      };
      next.set(providerId, { ...existing, baseUrl: url });
      return next;
    });
  };

  const toggleAdvanced = (providerId: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? {
        apiKey: "",
        baseUrl: "",
        expanded: false,
        showAdvanced: false,
      };
      next.set(providerId, { ...existing, showAdvanced: !existing.showAdvanced });
      return next;
    });
  };

  const handleProviderChange = async (providerId: string) => {
    setActiveProvider(providerId);
    const models = await invoke<string[]>("get_models_for_provider", {
      providerSlug: providerId,
    });
    if (models.length > 0) {
      setSelectedModel(models[0]);
    }
  };

  const [availableModels, setAvailableModels] = useState<string[]>([]);

  useEffect(() => {
    if (activeProvider) {
      invoke<string[]>("get_models_for_provider", {
        providerSlug: activeProvider,
      })
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
      <div>
        <h2 className="text-base font-semibold mb-1">Settings</h2>
        <p className="text-sm text-muted-foreground">
          Loading engine configuration...
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-8">
      {tab === "general" && (
        <section>
          <h2 className="text-base font-semibold mb-1">General</h2>
          <p className="text-sm text-muted-foreground mb-4">
            Configure prompt templates and output behavior.
          </p>
          <div className="space-y-2">
            <Label htmlFor="prompt-template-select">Prompt template</Label>
            <select
              id="prompt-template-select"
              className="h-9 w-full rounded-md border border-input bg-background px-3"
              value={promptTemplate}
              onChange={(e) => setPromptTemplate(e.target.value)}
            >
              {(config.prompt_template_options ?? ["General"]).map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </div>
        </section>
      )}

      {tab === "ai" && (
        <section className="space-y-8">
          {/* Active Provider & Model Selector */}
          <div>
            <h2 className="text-base font-semibold mb-1">AI provider</h2>
            <p className="text-sm text-muted-foreground mb-4">
              Select the provider and model for document parsing.
            </p>
            <div className="grid gap-4 grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="active-provider">Provider</Label>
                <select
                  id="active-provider"
                  className="h-9 w-full rounded-md border border-input bg-background px-3"
                  value={activeProvider}
                  onChange={(e) => void handleProviderChange(e.target.value)}
                >
                  {config.provider_options.map((opt) => (
                    <option key={opt.id} value={opt.id}>
                      {opt.label}
                    </option>
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
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </div>

          {/* Provider List */}
          <div>
            <h2 className="text-base font-semibold mb-4">Provider API keys</h2>
            <div className="space-y-2">
              {config.provider_options.map((opt) => {
                const state = providerStates.get(opt.id) ?? {
                  apiKey: "",
                  baseUrl: "",
                  expanded: false,
                  showAdvanced: false,
                };
                return (
                  <div
                    key={opt.id}
                    className="rounded-lg border bg-card text-card-foreground"
                  >
                    <div
                      className="flex cursor-pointer items-center justify-between px-3 h-10"
                      onClick={() => toggleExpanded(opt.id)}
                      role="button"
                      tabIndex={0}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ")
                          toggleExpanded(opt.id);
                      }}
                    >
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium leading-none">
                          {opt.label}
                        </span>
                        {activeProvider === opt.id && (
                          <span className="rounded-full bg-emerald-100 px-1.5 py-0 text-[10px] font-medium text-emerald-700 leading-none">
                            Active
                          </span>
                        )}
                      </div>
                      {state.expanded ? (
                        <ChevronDown
                          size={12}
                          className="text-muted-foreground shrink-0"
                        />
                      ) : (
                        <ChevronRight
                          size={12}
                          className="text-muted-foreground shrink-0"
                        />
                      )}
                    </div>
                    {state.expanded && (
                      <div className="border-t px-3 py-2">
                        <div className="flex items-center gap-3">
                          <Label className="text-xs whitespace-nowrap leading-none text-muted-foreground shrink-0">
                            API Key
                          </Label>
                          <Input
                            autoComplete="off"
                            onChange={(e) =>
                              updateApiKey(opt.id, e.target.value)
                            }
                            placeholder={
                              opt.requires_api_key ? "Required" : "Optional"
                            }
                            type="password"
                            value={state.apiKey}
                            className="h-7 text-xs min-w-0"
                          />
                        </div>
                        {opt.supports_base_url && (
                          <>
                            <button
                              type="button"
                              onClick={() => toggleAdvanced(opt.id)}
                              className="mt-1.5 flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                            >
                              {state.showAdvanced ? (
                                <ChevronDown size={12} />
                              ) : (
                                <ChevronRight size={12} />
                              )}
                              Advanced
                            </button>
                            {state.showAdvanced && (
                              <div className="flex items-center gap-2 mt-1.5">
                                <Label className="text-xs whitespace-nowrap leading-none text-muted-foreground shrink-0">
                                  Base URL
                                </Label>
                                <Input
                                  autoComplete="off"
                                  onChange={(e) =>
                                    updateBaseUrl(opt.id, e.target.value)
                                  }
                                  placeholder={
                                    opt.id === "ollama"
                                      ? "http://localhost:11434"
                                      : opt.id === "open_router"
                                      ? "https://openrouter.ai/v1"
                                      : "Optional"
                                  }
                                  type="text"
                                  value={state.baseUrl}
                                  className="h-7 text-xs min-w-0"
                                />
                              </div>
                            )}
                          </>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}

export function App() {
  const [snapshot, setSnapshot] = useState<UiSnapshot>(EMPTY_SNAPSHOT);
  const [loadedWorkspaceProject, setLoadedWorkspaceProject] =
    useState<WorkspaceProject | null>(null);
  const [currentConfig, setCurrentConfig] =
    useState<EngineConfigPayload | null>(null);
  const [validation, setValidation] =
    useState<ValidateProviderResponseData | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileSelection | null>(null);
  const [activePanel, setActivePanel] = useState<ActivePanel>("import");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("ai");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(true);
  const [startupError, setStartupError] = useState<string | null>(null);
  const previewWorkspaceProject = buildWorkspacePreview(
    snapshot.lastResult,
    Boolean(snapshot.activeJob),
  );
  const workspaceProject = loadedWorkspaceProject
    ? {
        ...loadedWorkspaceProject,
        summary: {
          ...loadedWorkspaceProject.summary,
          stale:
            loadedWorkspaceProject.summary.stale || Boolean(snapshot.activeJob),
        },
      }
    : previewWorkspaceProject;
  const [workspaceUiState, dispatchWorkspaceUi] = useReducer(
    workspaceUiStateReducer,
    null,
    createInitialWorkspaceUiState,
  );

  useEffect(() => {
    let unlisten: TauriUnlisten | null = null;

    const bootstrap = async () => {
      const tauri = getTauri();
      const [initialSnapshot, initialConfig, initialValidation] =
        await Promise.all([
          invoke<UiSnapshot>("app_snapshot"),
          invoke<EngineConfigPayload>("load_engine_config"),
          invoke<ValidateProviderResponseData>("validate_engine_config"),
        ]);
      const initialWorkspaceProject =
        await invoke<WorkspaceProject | null>("load_workspace_project");
      setSnapshot(initialSnapshot);
      setCurrentConfig(initialConfig);
      setValidation(initialValidation);
      setLoadedWorkspaceProject(initialWorkspaceProject);

      unlisten = await tauri.event.listen<UiSnapshot>(
        "duckdocs://snapshot",
        (message) => {
          setSnapshot(message.payload);
        },
      );
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

  useEffect(() => {
    let cancelled = false;

    if (snapshot.lastProjectId) {
      invoke<WorkspaceProject | null>("load_workspace_project", {
        project_id: snapshot.lastProjectId,
      })
        .then((project) => {
          if (!cancelled) {
            setLoadedWorkspaceProject(project);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setLoadedWorkspaceProject(null);
          }
        });
      return () => {
        cancelled = true;
      };
    }

    if (snapshot.lastResult) {
      setLoadedWorkspaceProject(null);
    }

    return () => {
      cancelled = true;
    };
  }, [snapshot.lastProjectId, snapshot.lastResult?.savedOutputPath]);

  useEffect(() => {
    dispatchWorkspaceUi({
      type: "sync_project",
      project: workspaceProject,
    });
  }, [
    workspaceProject?.summary.projectId,
    workspaceProject?.summary.stale,
    workspaceProject?.summary.nodeCount,
  ]);

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

  const applyWorkspaceCorrection = async (
    request: WorkspaceApplyCorrectionRequest,
  ) => {
    const project = await invoke<WorkspaceProject>("apply_workspace_correction", {
      correction: {
        projectId: request.projectId,
        nodeId: request.nodeId,
        kind: request.kind,
        targetNodeId: request.targetNodeId ?? null,
        value: request.value ?? null,
      },
    });
    setLoadedWorkspaceProject(project);
  };

  const answerWorkspaceProject = async (
    request: WorkspaceAnswerProjectRequest,
  ) => {
    return invoke<WorkspaceProject["answerByNodeId"][string]>(
      "answer_workspace_project",
      {
        request: {
          projectId: request.projectId,
          nodeId: request.nodeId ?? null,
          question: request.question,
        },
      },
    );
  };

  const saveConfig = async (payload: EngineConfigPayload) => {
    const saved = await invoke<EngineConfigPayload>("save_engine_config", {
      payload,
    });
    const nextValidation = await invoke<ValidateProviderResponseData>(
      "validate_engine_config",
      { payload: saved },
    );
    setCurrentConfig(saved);
    setValidation(nextValidation);
  };

  const validateConfig = async (payload: EngineConfigPayload | null) => {
    const nextValidation = await invoke<ValidateProviderResponseData>(
      "validate_engine_config",
      { payload },
    );
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

  const showSidebar = !sidebarCollapsed;

  function openSettings() {
    setSettingsOpen(true);
    setSidebarCollapsed(false);
  }

  function closeSettings() {
    setSettingsOpen(false);
    setActivePanel("import");
  }

  function openImportPanel() {
    setSettingsOpen(false);
    setActivePanel("import");
  }

  return (
    <main className="flex h-screen w-screen overflow-hidden bg-sidebar text-foreground">
      {/* Sidebar — char-style: native titlebar, h-9 header, hide completely when collapsed */}
      {showSidebar && (
        <aside className="flex h-full w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar">
          {/* Header row matches macOS traffic-lights height, same as char */}
          <header
            data-tauri-drag-region
            className="flex h-9 w-full shrink-0 items-center justify-end pl-20 pr-2"
          >
            {!settingsOpen && (
              <Button
                aria-label="Collapse sidebar"
                onClick={() => setSidebarCollapsed(true)}
                size="icon"
                variant="ghost"
                className="size-7"
                type="button"
              >
                <PanelLeftClose size={14} />
              </Button>
            )}
            {settingsOpen && (
              <div className="flex items-center gap-0.5">
                <Button
                  aria-label="Back to import"
                  onClick={() => {
                    setSettingsOpen(false);
                    setActivePanel("import");
                  }}
                  size="icon"
                  variant="ghost"
                  className="size-7"
                  type="button"
                >
                  <ArrowLeft size={14} />
                </Button>
              </div>
            )}
          </header>

          {/* Navigation content */}
          <div className="flex min-h-0 flex-1 flex-col px-3 overflow-y-auto">
            <nav className="space-y-0.5">
              {!settingsOpen && (
                <>
                  {MAIN_NAV_ITEMS.map((item) => (
                    <Button
                      key={item.id}
                      aria-current={
                        activePanel === item.id ? "page" : undefined
                      }
                      className={sidebarButtonClass(activePanel === item.id)}
                      onClick={() => setActivePanel(item.id)}
                      size="sm"
                      variant={activePanel === item.id ? "secondary" : "ghost"}
                      type="button"
                    >
                      <span aria-hidden="true">{item.icon}</span>
                      <span className="font-medium">{item.label}</span>
                    </Button>
                  ))}
                </>
              )}

              {settingsOpen && (
                <>
                  {SETTINGS_NAV_ITEMS.map((item) => (
                    <Button
                      key={item.id}
                      aria-current={
                        settingsTab === item.id ? "page" : undefined
                      }
                      className={sidebarButtonClass(settingsTab === item.id)}
                      onClick={() => setSettingsTab(item.id)}
                      size="sm"
                      variant={settingsTab === item.id ? "secondary" : "ghost"}
                      type="button"
                    >
                      <span aria-hidden="true">{item.icon}</span>
                      <span className="font-medium">{item.label}</span>
                    </Button>
                  ))}
                </>
              )}
            </nav>

            {!settingsOpen && (
              <div className="mt-auto pb-6 pt-2">
                <Button
                  className={sidebarButtonClass(false)}
                  onClick={() => openSettings()}
                  size="sm"
                  variant="ghost"
                  type="button"
                >
                  <span aria-hidden="true">
                    <Settings size={18} />
                  </span>
                  <span className="font-medium">Settings</span>
                </Button>
              </div>
            )}
          </div>
        </aside>
      )}

      {/* Main content area */}
      <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
        {!showSidebar && (
          <header
            data-tauri-drag-region
            className="flex h-9 w-full shrink-0 items-center pl-20 pr-2"
          >
            <Button
              aria-label="Expand sidebar"
              onClick={() => setSidebarCollapsed(false)}
              size="icon"
              variant="ghost"
              className="size-7"
              type="button"
            >
              <PanelLeftOpen size={14} />
            </Button>
          </header>
        )}

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
          {settingsOpen ? (
            <SettingsPanel
              config={currentConfig}
              onSave={saveConfig}
              onValidate={validateConfig}
              validation={validation}
              tab={settingsTab}
              onTabChange={setSettingsTab}
            />
          ) : activePanel === "graph" ? (
            <GraphWorkspace
              dispatch={dispatchWorkspaceUi}
              onApplyCorrection={applyWorkspaceCorrection}
              onAskProject={answerWorkspaceProject}
              onOpenImport={openImportPanel}
              project={workspaceProject}
              uiState={workspaceUiState}
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
        </div>
      </section>
    </main>
  );
}
