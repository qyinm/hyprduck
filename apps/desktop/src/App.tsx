import {
  Component,
  type ErrorInfo,
  type ReactNode,
  useEffect,
  useReducer,
  useState,
} from "react";
import {
  ArrowLeft,
  Bell,
  BookOpen,
  ChevronDown,
  ChevronRight,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Save,
  Settings,
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

type ActivePanel = "knowledge" | "settings";
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

interface DesktopMessage<T> {
  payload: T;
}

type DesktopUnlisten = () => void;

interface HyprDuckDesktopApi {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(
    eventName: string,
    handler: (message: DesktopMessage<T>) => void | Promise<void>,
  ): DesktopUnlisten;
}

declare global {
  interface Window {
    duckdocs?: HyprDuckDesktopApi;
  }
}

const IS_WEB_PREVIEW = import.meta.env.VITE_PLATFORM === "web";

const WEB_MOCK_PROVIDER_OPTIONS: ProviderOption[] = [
  {
    id: "open_router",
    label: "OpenRouter",
    requires_api_key: true,
    supports_base_url: true,
  },
  {
    id: "openai",
    label: "OpenAI",
    requires_api_key: true,
    supports_base_url: false,
  },
  {
    id: "anthropic",
    label: "Anthropic",
    requires_api_key: true,
    supports_base_url: false,
  },
  {
    id: "ollama",
    label: "Ollama",
    requires_api_key: false,
    supports_base_url: true,
  },
];

const WEB_MOCK_CONFIG: EngineConfigPayload = {
  provider: "ollama",
  model_id: "llama3.1",
  api_key: "",
  base_url: "http://localhost:11434",
  prompt_template: "General",
  provider_options: WEB_MOCK_PROVIDER_OPTIONS,
  model_options: ["llama3.1", "llava:latest", "gpt-4o-mini"],
  prompt_template_options: [
    "General",
    "Tutorial",
    "UI flow",
    "Code",
    "Table",
  ],
};

const WEB_MOCK_SAMPLE_FILE: FileSelection = {
  path: "/tmp/duckdocs-sample.pdf",
  format: "pdf",
};

const WEB_MOCK_MARKDOWN = `# Sample import

## Page 1
This is a demonstration preview run in the browser.

## Page 2
The real Electron runtime is not available in this preview, so we show read-only sample behavior.
`;

const WEB_MOCK_BASE_SNAPSHOT: UiSnapshot = {
  activeJob: null,
  progressLog: [
    {
      phase: "import",
      message: "Desktop runtime not detected. Running in browser preview mode.",
      timestamp: new Date().toISOString(),
    },
  ],
  lastResult: {
    savedOutputPath: "~/Documents/HyprDuck/web-preview/sample.md",
    successCount: 2,
    failedCount: 0,
    markdown: WEB_MOCK_MARKDOWN,
  },
  lastProjectId: "preview:sample",
};

const WEB_MOCK_PROVIDER_MODELS: Record<string, string[]> = {
  open_router: ["gpt-4o", "claude-3.5-sonnet", "llama-3.1-70b"],
  openai: ["gpt-4o-mini", "gpt-4.1-mini"],
  anthropic: ["claude-3.5-sonnet", "claude-3.7-sonnet"],
  ollama: ["llama3.1", "llava:latest", "qwen2.5vl"],
};

let webMockSnapshot = WEB_MOCK_BASE_SNAPSHOT;
let webMockConfig: EngineConfigPayload = WEB_MOCK_CONFIG;
let webMockValidation: ValidateProviderResponseData = { ready: false, issues: [] };
let webMockParseTimer: ReturnType<typeof setTimeout> | null = null;
const webMockSnapshotListeners = new Set<
  (message: DesktopMessage<UiSnapshot>) => void
>();

function deriveWebValidation(
  payload: EngineConfigPayload | null,
): ValidateProviderResponseData {
  const config = payload ?? webMockConfig;
  const provider = WEB_MOCK_PROVIDER_OPTIONS.find(
    (option) => option.id === config.provider,
  );
  const issues: ValidationIssue[] = [];
  if (provider?.requires_api_key && !config.api_key.trim()) {
    issues.push({ message: `${provider.label} requires an API key.` });
  }
  return {
    ready: issues.length === 0,
    issues,
  };
}

function emitWebSnapshot(snapshot: UiSnapshot) {
  webMockSnapshot = snapshot;
  const payload: DesktopMessage<UiSnapshot> = { payload: snapshot };
  for (const listener of webMockSnapshotListeners) {
    void Promise.resolve()
      .then(() => listener(payload))
      .catch((error: unknown) => {
        console.error("Web mock listener error:", error);
      });
  }
}

function getWebWorkspaceFromSnapshot(
  snapshot: UiSnapshot = webMockSnapshot,
): WorkspaceProject | null {
  if (!snapshot.lastResult) {
    return null;
  }
  return buildWorkspacePreview(snapshot.lastResult, Boolean(snapshot.activeJob));
}

function createWebMockApi(): HyprDuckDesktopApi {
  webMockValidation = deriveWebValidation(null);

  return {
    async invoke<T>(
      command: string,
      args: Record<string, unknown> = {},
    ): Promise<T> {
      switch (command) {
        case "app_snapshot": {
          return { ...webMockSnapshot } as T;
        }
        case "load_engine_config": {
          return { ...webMockConfig, provider_options: [...WEB_MOCK_PROVIDER_OPTIONS] } as T;
        }
        case "validate_engine_config": {
          const next = deriveWebValidation(
            (args.payload as { payload?: EngineConfigPayload } | undefined)
              ?.payload ?? null,
          );
          webMockValidation = next;
          return { ...next } as T;
        }
        case "get_models_for_provider": {
          const key = String(
            (args.providerSlug as string | undefined) ??
              webMockConfig.provider ??
              "ollama",
          );
          return [...(WEB_MOCK_PROVIDER_MODELS[key] ?? WEB_MOCK_PROVIDER_MODELS.ollama)] as T;
        }
        case "load_workspace_project": {
          const projectId = args.project_id as string | undefined;
          const project = getWebWorkspaceFromSnapshot();
          if (!project || (projectId && project.summary.projectId !== projectId)) {
            return null as T;
          }
          return { ...project } as T;
        }
        case "pick_import_file": {
          return { ...WEB_MOCK_SAMPLE_FILE } as T;
        }
        case "start_parse": {
          const request =
            (args.request as { path?: string; format?: string } | undefined) ??
            null;
          const filePath = request?.path ?? WEB_MOCK_SAMPLE_FILE.path;
          const format = request?.format ?? WEB_MOCK_SAMPLE_FILE.format;
          const started: ActiveJobSnapshot = {
            jobId: `preview-${Date.now()}`,
            filePath,
            format,
            status: "running",
            progressPercent: 0,
            lastMessage: "Preview parse started.",
          };
          if (webMockParseTimer) {
            clearTimeout(webMockParseTimer);
          }
          emitWebSnapshot({
            ...webMockSnapshot,
            activeJob: started,
            lastResult: webMockSnapshot.lastResult,
            progressLog: [
              ...webMockSnapshot.progressLog,
              {
                phase: "parse",
                message: "Using mocked web preview parser.",
                timestamp: new Date().toISOString(),
              },
            ],
          });
          webMockParseTimer = setTimeout(() => {
            const completedSnapshot: UiSnapshot = {
              ...webMockSnapshot,
              activeJob: null,
              lastProjectId: "preview:sample",
              lastResult: {
                savedOutputPath: `~/Documents/HyprDocs/web-preview/${new Date()
                  .toISOString()
                  .slice(0, 10)}.md`,
                successCount: 2,
                failedCount: 0,
                markdown: WEB_MOCK_MARKDOWN,
              },
              progressLog: [
                ...webMockSnapshot.progressLog,
                {
                  phase: "parse",
                  message: "Preview parse completed.",
                  timestamp: new Date().toISOString(),
                },
              ],
            };
            emitWebSnapshot(completedSnapshot);
            webMockParseTimer = null;
          }, 700);
          return undefined as T;
        }
        case "cancel_parse": {
          if (webMockParseTimer) {
            clearTimeout(webMockParseTimer);
            webMockParseTimer = null;
          }
          const current = webMockSnapshot;
          if (current.activeJob) {
            emitWebSnapshot({
              ...current,
              activeJob: null,
              progressLog: [
                ...current.progressLog,
                {
                  phase: "parse",
                  message: "Preview parse canceled.",
                  timestamp: new Date().toISOString(),
                },
              ],
            });
          }
          return undefined as T;
        }
        case "open_saved_output": {
          const path = String((args.path as string | undefined) ?? "");
          if (typeof window !== "undefined") {
            window.alert(`Cannot open local files from web preview: ${path}`);
          }
          return undefined as T;
        }
        case "apply_workspace_correction": {
          const workspace = getWebWorkspaceFromSnapshot();
          if (!workspace) {
            throw new Error("No workspace available in preview mode.");
          }
          return { ...workspace } as T;
        }
        case "answer_workspace_project": {
          const request = args.request as
            | WorkspaceAnswerProjectRequest
            | undefined;
          const workspace = getWebWorkspaceFromSnapshot();
          if (!workspace) {
            throw new Error("No workspace available in preview mode.");
          }
          const answer = request?.nodeId
            ? workspace.answerByNodeId[request.nodeId]
            : workspace.answerByNodeId.document;
          if (!answer) {
            throw new Error("No answer available for this node in preview mode.");
          }
          return { ...answer } as T;
        }
        case "save_engine_config": {
          const payload = args.payload as EngineConfigPayload;
          webMockConfig = {
            ...webMockConfig,
            ...payload,
            provider_options: WEB_MOCK_PROVIDER_OPTIONS,
          };
          return { ...webMockConfig } as T;
        }
        default:
          throw new Error(`web-preview: unsupported command "${command}".`);
      }
    },
    listen<T>(
      eventName: string,
      handler: (message: DesktopMessage<T>) => void | Promise<void>,
    ): DesktopUnlisten {
      if (eventName !== "duckdocs://snapshot") {
        return () => undefined;
      }
      const typedHandler = (message: DesktopMessage<UiSnapshot>) => {
        void handler(message as DesktopMessage<T>);
      };
      webMockSnapshotListeners.add(typedHandler);
      return () => {
        webMockSnapshotListeners.delete(typedHandler);
      };
    },
  };
}

const webPreviewApi = IS_WEB_PREVIEW ? createWebMockApi() : null;

const MAIN_NAV_ITEMS: { id: ActivePanel; label: string; icon: ReactNode }[] = [
  {
    id: "knowledge",
    label: "Knowledge",
    icon: <BookOpen aria-hidden="true" size={18} />,
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

class WorkspaceErrorBoundary extends Component<
  { children: ReactNode },
  { errorMessage: string | null }
> {
  constructor(props: { children: ReactNode }) {
    super(props);
    this.state = { errorMessage: null };
  }

  static getDerivedStateFromError(error: unknown) {
    return {
      errorMessage:
        error instanceof Error ? error.message : "Unknown workspace render error",
    };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("Workspace render failed", error, info);
  }

  render() {
    if (this.state.errorMessage) {
      return (
        <div className="flex min-h-[24rem] flex-col items-center justify-center rounded-[24px] border border-red-200 bg-red-50/80 p-8 text-center">
          <h2 className="text-lg font-semibold text-red-900">
            Graph workspace failed to render
          </h2>
          <p className="mt-3 max-w-2xl text-sm leading-6 text-red-800">
            HyprDuck hit a frontend render error instead of showing the graph.
            The latest issue is:
          </p>
          <pre className="mt-4 max-w-3xl overflow-x-auto rounded-2xl bg-white/90 px-4 py-3 text-left text-xs leading-6 text-red-900">
            {this.state.errorMessage}
          </pre>
        </div>
      );
    }

    return this.props.children;
  }
}

function getDesktopApi(): HyprDuckDesktopApi {
  if (IS_WEB_PREVIEW) {
    return webPreviewApi as HyprDuckDesktopApi;
  }
  const api = window.duckdocs;
  if (!api) {
    throw new Error("HyprDuck desktop UI requires Electron preload APIs.");
  }
  return api;
}

async function invoke<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return getDesktopApi().invoke<T>(command, args);
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
    "h-9 w-full justify-start gap-3 rounded-full border px-3 text-sm font-medium",
    active
      ? "border-border bg-secondary text-foreground"
      : "border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground",
  );
}

function windowChromeButtonClass(): string {
  return "h-7 w-7 rounded-full border border-transparent bg-background/80 text-muted-foreground shadow-none backdrop-blur hover:border-border hover:bg-secondary hover:text-foreground";
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

  // Auto-save whenever settings change
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!config) return;
    const timer = setTimeout(() => {
      const activeState = providerStates.get(activeProvider);
      const payload: EngineConfigPayload = {
        provider: activeProvider,
        model_id: selectedModel,
        api_key: activeApiKey,
        base_url: activeState?.baseUrl || null,
        prompt_template: promptTemplate,
        provider_options: config?.provider_options ?? [],
        model_options: availableModels,
        prompt_template_options: config?.prompt_template_options ?? [],
      };
      setSaving(true);
      onSave(payload).finally(() => setSaving(false));
    }, 600);
    return () => clearTimeout(timer);
  }, [activeProvider, selectedModel, activeApiKey, providerStates, promptTemplate]);

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
                          <span className="rounded-full border border-border bg-secondary px-1.5 py-0 text-[10px] font-medium leading-none text-foreground">
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
  const [activePanel, setActivePanel] = useState<ActivePanel>("knowledge");
  const settingsOpen = activePanel === "settings";
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("ai");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(true);
  const [healthOpen, setHealthOpen] = useState(false);
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
    let unlisten: DesktopUnlisten | null = null;

    const bootstrap = async () => {
      const desktop = getDesktopApi();
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

      unlisten = desktop.listen<UiSnapshot>("duckdocs://snapshot", (message) => {
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
      setActivePanel("knowledge");
      try {
        await invoke<void>("start_parse", {
          request: {
            path: selection.path,
            format: selection.format,
          },
        });
      } catch (error) {
        setSelectedFile(null);
        window.alert(`Failed to start parsing: ${String(error)}`);
      }
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
    setActivePanel("knowledge");
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
            <CardTitle>HyprDuck failed to start</CardTitle>
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
    setActivePanel("settings");
    setSidebarCollapsed(false);
  }

  function closeSettings() {
    setActivePanel("knowledge");
  }

  return (
    <main className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <div
        data-electron-drag-region
        className="fixed inset-x-0 top-0 z-40 h-12"
      />
      <div
        data-electron-no-drag
        className="fixed left-[88px] top-[10px] z-50 flex h-7 items-center gap-1"
      >
        {settingsOpen ? (
          <Button
            aria-label="Back to Knowledge"
            onClick={() => {
              setActivePanel("knowledge");
              setHealthOpen(false);
            }}
            size="icon"
            variant="ghost"
            className={windowChromeButtonClass()}
            type="button"
          >
            <ArrowLeft size={14} />
          </Button>
        ) : showSidebar ? (
          <Button
            aria-label="Collapse sidebar"
            onClick={() => {
              setSidebarCollapsed(true);
              setHealthOpen(false);
            }}
            size="icon"
            variant="ghost"
            className={windowChromeButtonClass()}
            type="button"
          >
            <PanelLeftClose size={14} />
          </Button>
        ) : (
          <Button
            aria-label="Expand sidebar"
            onClick={() => {
              setSidebarCollapsed(false);
              setHealthOpen(false);
            }}
            size="icon"
            variant="ghost"
            className={windowChromeButtonClass()}
            type="button"
          >
            <PanelLeftOpen size={14} />
          </Button>
        )}
      </div>
      <Button
        aria-expanded={healthOpen}
        aria-label="Knowledge maintenance"
        title="Knowledge maintenance"
        data-electron-no-drag
        onClick={() => setHealthOpen((open) => !open)}
        size="icon"
        variant="ghost"
        className={cn(
          "fixed top-[10px] z-50",
          !settingsOpen && workspaceUiState.inspectorOpen ? "" : "right-12",
          windowChromeButtonClass(),
        )}
        style={
          !settingsOpen && workspaceUiState.inspectorOpen
            ? { right: "calc(clamp(18rem, 28vw, 24rem) + 0.75rem)" }
            : undefined
        }
        type="button"
      >
        <Bell size={14} />
      </Button>
      {!settingsOpen && (
        <Button
          aria-expanded={workspaceUiState.inspectorOpen}
          aria-label={
            workspaceUiState.inspectorOpen
              ? "Collapse right inspector"
              : "Expand right inspector"
          }
          title={
            workspaceUiState.inspectorOpen
              ? "Collapse right inspector"
              : "Expand right inspector"
          }
          data-electron-no-drag
          onClick={() => {
            dispatchWorkspaceUi({ type: "toggle_inspector" });
            setHealthOpen(false);
          }}
          size="icon"
          variant="ghost"
          className={cn("fixed right-3 top-[10px] z-50", windowChromeButtonClass())}
          type="button"
        >
          {workspaceUiState.inspectorOpen ? (
            <PanelRightClose size={14} />
          ) : (
            <PanelRightOpen size={14} />
          )}
        </Button>
      )}
      {healthOpen && (
        <section
          data-electron-no-drag
          className={cn(
            "fixed top-12 z-50 w-72 rounded-xl border border-border bg-background p-4 text-sm shadow-none",
            !settingsOpen && workspaceUiState.inspectorOpen ? "" : "right-3",
          )}
          style={
            !settingsOpen && workspaceUiState.inspectorOpen
              ? { right: "calc(clamp(18rem, 28vw, 24rem) + 0.75rem)" }
              : undefined
          }
        >
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold text-foreground">Knowledge maintenance</h2>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                Safe ingest repairs run automatically. Only conflicts, failed writes, or risky merges need review.
              </p>
            </div>
            <span className="rounded-full border border-border bg-secondary px-2 py-1 text-[11px] font-medium text-foreground">
              Quiet
            </span>
          </div>
          <div className="mt-4 rounded-xl border border-border bg-secondary/60 p-3 text-xs leading-5 text-muted-foreground">
            No user action needed. The local knowledge base is ready for source updates and grounded answers.
          </div>
        </section>
      )}
      {/* Sidebar — native titlebar area stays empty; chrome controls are fixed to the window */}
      {showSidebar && (
        <aside className="flex h-full w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar">
          <div className="h-12 w-full shrink-0" />

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
        <div
          className={cn(
            "flex min-h-0 flex-1 flex-col overflow-hidden",
            settingsOpen ? "p-6 pt-14" : "",
          )}
        >
          {settingsOpen ? (
            <SettingsPanel
              config={currentConfig}
              onSave={saveConfig}
              onValidate={validateConfig}
              validation={validation}
              tab={settingsTab}
              onTabChange={setSettingsTab}
            />
          ) : activePanel === "knowledge" ? (
            <WorkspaceErrorBoundary>
              <GraphWorkspace
                dispatch={dispatchWorkspaceUi}
                onApplyCorrection={applyWorkspaceCorrection}
                onAskProject={answerWorkspaceProject}
                onOpenImport={chooseFile}
                project={workspaceProject}
                uiState={workspaceUiState}
              />
            </WorkspaceErrorBoundary>
          ) : null}
        </div>
      </section>
    </main>
  );
}
