import {
  Component,
  type ErrorInfo,
  type ReactNode,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  History as HistoryIcon,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  RefreshCw,
  Save,
  Settings,
  Sparkles,
} from "lucide-react";
import {
  type BrainEvent,
  type BrainHealthResponseData,
  type DesktopCommand,
  type DesktopCommandArgs,
  type DesktopCommandParameters,
  type DesktopCommandResult,
  type DesktopMessage,
  type DesktopUnlisten,
  type EngineConfigPayload,
  type FileSelection,
  type HyprDuckDesktopApi,
  type UiSnapshot,
  type ValidateProviderResponseData,
  type RuntimeReadinessResponseData,
  type WorkspaceLoadResult,
  type WorkspaceLoadState,
} from "@/appTypes";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { GraphWorkspace } from "@/features/workspace/GraphWorkspace";
import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import { materializedGraphSnapshotToWorkspaceEnvelope } from "@/features/workspace/materializedGraphSnapshot";
import {
  createInitialWorkspaceUiState,
  workspaceUiStateReducer,
} from "@/features/workspace/state";
import type {
  MaterializedGraphSnapshot,
  WorkspaceApplyCorrectionRequest,
  WorkspaceAnswerProjectRequest,
  WorkspaceProjectEnvelope,
  WorkspaceProject,
} from "@/features/workspace/types";
import { cn } from "@/lib/utils";
import { createWebMockApi } from "@/webPreviewApi";
import {
  createEmptyWorkspaceProject,
  hydrateWorkspaceProjectWithSources,
} from "@/workspaceSourceHydration";

type ActivePanel = "knowledge" | "settings";
type SettingsTab = "general" | "ai";

declare global {
  interface Window {
    hyprduck?: HyprDuckDesktopApi;
  }
}

const IS_WEB_PREVIEW = import.meta.env.VITE_PLATFORM === "web";

const webPreviewApi = IS_WEB_PREVIEW ? createWebMockApi() : null;

const MAIN_NAV_ITEMS: { id: ActivePanel; label: string; icon: ReactNode }[] = [
  {
    id: "knowledge",
    label: "Knowledge",
    icon: (
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width={18}
        height={18}
        viewBox="0 0 256 256"
        aria-hidden="true"
      >
        <path
          fill="currentColor"
          d="M200 152a31.84 31.84 0 0 0-19.53 6.68l-23.11-18A31.65 31.65 0 0 0 160 128c0-.74 0-1.48-.08-2.21l13.23-4.41A32 32 0 1 0 168 104c0 .74 0 1.48.08 2.21l-13.23 4.41A32 32 0 0 0 128 96a32.6 32.6 0 0 0-5.27.44L115.89 81A32 32 0 1 0 96 88a32.6 32.6 0 0 0 5.27-.44l6.84 15.4a31.92 31.92 0 0 0-8.57 39.64l-25.71 22.84a32.06 32.06 0 1 0 10.63 12l25.71-22.84a31.91 31.91 0 0 0 37.36-1.24l23.11 18A31.65 31.65 0 0 0 168 184a32 32 0 1 0 32-32m0-64a16 16 0 1 1-16 16a16 16 0 0 1 16-16M80 56a16 16 0 1 1 16 16a16 16 0 0 1-16-16M56 208a16 16 0 1 1 16-16a16 16 0 0 1-16 16m56-80a16 16 0 1 1 16 16a16 16 0 0 1-16-16m88 72a16 16 0 1 1 16-16a16 16 0 0 1-16 16"
        />
      </svg>
    ),
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
  workspaceRevision: 0,
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
  const api = window.hyprduck;
  if (!api) {
    throw new Error("HyprDuck desktop UI requires Electron preload APIs.");
  }
  return api;
}

async function invoke<K extends DesktopCommand>(
  command: K,
  ...args: DesktopCommandParameters<K>
): Promise<DesktopCommandResult<K>> {
  return getDesktopApi().invoke(command, ...args);
}

async function loadGraphWorkspaceEnvelope(
  workspaceId?: string | null,
  projectId?: string | null,
): Promise<WorkspaceProjectEnvelope> {
  return (await loadGraphWorkspaceEnvelopeResult(workspaceId, projectId)).envelope;
}

async function loadGraphWorkspaceEnvelopeResult(
  workspaceId?: string | null,
  projectId?: string | null,
): Promise<WorkspaceLoadResult> {
  try {
    const materializedSnapshot = await invoke("load_materialized_graph_snapshot", {
      workspace_id: workspaceId ?? undefined,
    });
    return {
      envelope: materializedGraphSnapshotToWorkspaceEnvelope(materializedSnapshot),
      source: "materialized",
    };
  } catch (materializedError) {
    try {
      return {
        envelope: await invoke("load_workspace_project", {
          project_id: projectId ?? null,
          workspace_id: workspaceId ?? null,
        }),
        source: "legacy",
        fallbackReason: String(materializedError),
      };
    } catch (legacyError) {
      throw new Error(
        `Failed to refresh latest workspace snapshot. Materialized read failed: ${String(
          materializedError,
        )}. Legacy project read failed: ${String(legacyError)}.`,
      );
    }
  }
}

function workspaceLoadStateFromResult(result: WorkspaceLoadResult): WorkspaceLoadState {
  if (result.source === "materialized") {
    const snapshotPath =
      result.envelope.project?.summary.summary.match(/from (.+)\.$/)?.[1] ??
      "state/latest-readable-snapshot.json";
    return {
      status: "ready",
      message: `Loaded latest materialized snapshot from ${snapshotPath}.`,
    };
  }

  return {
    status: "fallback",
    message:
      "Materialized snapshot was unavailable, so HyprDuck loaded the legacy workspace project read path.",
  };
}

function WorkspaceSnapshotStatusBanner({
  state,
}: {
  state: WorkspaceLoadState;
}) {
  if (state.status === "idle" || state.status === "ready") {
    return null;
  }

  const title =
    state.status === "loading"
      ? "Refreshing latest workspace snapshot"
      : state.status === "fallback"
        ? "Loaded legacy workspace project read path"
        : "Could not refresh the workspace snapshot";
  const tone =
    state.status === "error"
      ? "border-destructive/30 bg-destructive/5 text-destructive"
      : "border-border bg-background/90 text-muted-foreground";

  return (
    <div className="pointer-events-none fixed left-1/2 top-12 z-40 w-[min(36rem,calc(100vw-2rem))] -translate-x-1/2 px-2">
      <div className={cn("rounded-md border px-3 py-2 text-xs shadow-sm", tone)}>
        <span className="font-medium text-foreground">{title}</span>
        {state.message ? <span className="ml-2">{state.message}</span> : null}
      </div>
    </div>
  );
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

function modelTaskGuidance(providerId: string, modelId: string) {
  const model = modelId.toLowerCase();

  if (providerId === "ollama") {
    if (
      model.includes("8b") ||
      model.includes("ocr") ||
      model.includes("llama3.1")
    ) {
      return {
        tone: "warning",
        title: "Local model caution",
        body: "This keeps data local, but small or OCR-only models can miss tables, conflicts, and evidence links. Run the golden corpus before relying on agent-ready outputs.",
      };
    }

    return {
      tone: "local",
      title: "Local-first path",
      body: "Good for private parsing and retrieval checks. Keep generated merge output disabled until the golden corpus is clean.",
    };
  }

  return {
    tone: "hosted",
    title: "Hosted quality path",
    body: "Recommended for high-recall page parsing, structured extraction, and merge verification when privacy policy allows hosted inference.",
  };
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
  readiness: RuntimeReadinessResponseData | null;
  onSave: (payload: EngineConfigPayload) => Promise<void>;
  onValidate: (payload: EngineConfigPayload | null) => Promise<void>;
  onRefreshReadiness: () => Promise<void>;
  tab: SettingsTab;
  onTabChange: (tab: SettingsTab) => void;
}) {
  const {
    config,
    validation,
    readiness,
    onSave,
    onValidate,
    onRefreshReadiness,
    tab,
    onTabChange: setTab,
  } = props;
  const [promptTemplate, setPromptTemplate] = useState("General");
  const [selectedModel, setSelectedModel] = useState("");
  const [activeProvider, setActiveProvider] = useState("open_router");
  const [providerStates, setProviderStates] = useState<
    Map<string, ProviderState>
  >(new Map());
  const lastSavedSettingsSignature = useRef<string | null>(null);

  function settingsSignature(payload: {
    provider: string;
    model_id: string;
    api_key: string;
    base_url: string | null;
    prompt_template: string;
  }) {
    return JSON.stringify({
      provider: payload.provider,
      model_id: payload.model_id,
      api_key: payload.api_key,
      base_url: payload.base_url ?? null,
      prompt_template: payload.prompt_template,
    });
  }

  useEffect(() => {
    if (config) {
      setActiveProvider(config.provider);
      setSelectedModel(config.model_id);
      setPromptTemplate(config.prompt_template ?? "General");
      lastSavedSettingsSignature.current = settingsSignature(config);
      setProviderStates((prev) => {
        const next = new Map(prev);
        for (const opt of config.provider_options) {
          const existing = prev.get(opt.id);
          const isActive = opt.id === config.provider;
          next.set(opt.id, {
            apiKey: isActive ? config.api_key : existing?.apiKey ?? "",
            baseUrl: isActive ? config.base_url ?? "" : existing?.baseUrl ?? "",
            expanded: existing?.expanded ?? false,
            showAdvanced: existing?.showAdvanced ?? false,
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
    const models = await invoke("get_models_for_provider", {
      providerSlug: providerId,
    });
    if (models.length > 0) {
      setSelectedModel(models[0]);
    }
  };

  const [availableModels, setAvailableModels] = useState<string[]>([]);

  useEffect(() => {
    if (activeProvider) {
      invoke("get_models_for_provider", {
        providerSlug: activeProvider,
      })
        .then((models) => setAvailableModels(models))
        .catch(() => setAvailableModels([]));
    }
  }, [activeProvider]);

  const activeApiKey = providerStates.get(activeProvider)?.apiKey ?? "";
  const activeBaseUrl = providerStates.get(activeProvider)?.baseUrl ?? "";

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
      const nextSignature = settingsSignature(payload);
      if (nextSignature === lastSavedSettingsSignature.current) {
        return;
      }
      lastSavedSettingsSignature.current = nextSignature;
      setSaving(true);
      onSave(payload)
        .catch(() => {
          lastSavedSettingsSignature.current = null;
        })
        .finally(() => setSaving(false));
    }, 600);
    return () => clearTimeout(timer);
  }, [
    activeProvider,
    selectedModel,
    activeApiKey,
    activeBaseUrl,
    promptTemplate,
    availableModels,
    config,
  ]);

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

  const modelGuidance = modelTaskGuidance(activeProvider, selectedModel);

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
          <div>
            <div className="mb-3 flex items-start justify-between gap-4">
              <div>
                <h2 className="text-base font-semibold mb-1">Runtime readiness</h2>
                <p className="text-sm text-muted-foreground">
                  Local parser and provider checks for document processing.
                </p>
              </div>
              <Button
                onClick={() => void onRefreshReadiness()}
                size="sm"
                type="button"
                variant="outline"
              >
                Refresh
              </Button>
            </div>
            <div className="grid gap-2">
              {(readiness?.checks ?? []).map((check) => (
                <div
                  className="rounded-lg border border-border bg-secondary/50 p-3 text-xs leading-5"
                  key={check.id}
                >
                  <div className="flex items-center justify-between gap-3">
                    <span className="font-medium text-foreground">{check.label}</span>
                    <span
                      className={cn(
                        "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                        check.ready
                          ? "border-border text-foreground"
                          : check.required
                            ? "border-destructive/30 text-destructive"
                            : "border-border text-muted-foreground",
                      )}
                    >
                      {check.ready ? "Ready" : check.required ? "Issue" : "Optional"}
                    </span>
                  </div>
                  <p className="mt-1 text-muted-foreground">{check.message}</p>
                </div>
              ))}
              {!readiness && (
                <div className="rounded-lg border border-border bg-secondary/50 p-3 text-sm text-muted-foreground">
                  Runtime status is loading.
                </div>
              )}
            </div>
          </div>

          {/* Active Provider & Model Selector */}
          <div>
            <h2 className="text-base font-semibold mb-1">AI provider</h2>
            <p className="text-sm text-muted-foreground mb-4">
              Select the provider and model for parsing, extraction, merge
              verification, and grounded answer workflows.
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
            <div
              className={cn(
                "mt-3 rounded-lg border px-3 py-2 text-xs leading-5",
                modelGuidance.tone === "warning"
                  ? "border-amber-200 bg-amber-50 text-amber-900"
                  : "border-border bg-secondary/50 text-muted-foreground",
              )}
            >
              <div className="font-medium text-foreground">
                {modelGuidance.title}
              </div>
              <p>{modelGuidance.body}</p>
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

function HistoryPanel(props: {
  health: BrainHealthResponseData | null;
  onRefresh: () => Promise<void>;
}) {
  const { health, onRefresh } = props;
  const recentEvents = health?.recentEvents ?? [];
  const visibleEvents = recentEvents.filter(isHistoryActivityEvent);
  const hasActivity = visibleEvents.length > 0;

  return (
    <section
      aria-label="History"
      className="fixed right-3 top-12 z-50 flex max-h-[min(24rem,calc(100vh-4rem))] w-[min(26rem,calc(100vw-1.5rem))] flex-col overflow-hidden rounded-lg border border-border bg-background text-sm shadow-xl"
      data-electron-no-drag
    >
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <HistoryIcon className="shrink-0 text-muted-foreground" size={15} />
          <h2 className="truncate text-sm font-semibold text-foreground">
            History
          </h2>
        </div>
        <div className="flex shrink-0 items-center">
          <Button
            aria-label="Refresh history"
            onClick={() => void onRefresh()}
            size="icon"
            title="Refresh"
            type="button"
            variant="ghost"
          >
            <RefreshCw size={14} />
          </Button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex items-center justify-between gap-3 border-b border-border bg-secondary/20 px-3 py-2">
          <span className="text-xs font-medium text-muted-foreground">Recent activity</span>
          <span className="text-xs text-muted-foreground">
            {visibleEvents.length} events
          </span>
        </div>
        <div className="grid gap-1.5 p-2">
          {visibleEvents.map((event) => (
            <div
              className="rounded-md border border-border bg-background px-2.5 py-2"
              key={event.eventId}
            >
              <div className="flex min-w-0 items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-xs font-medium text-foreground">
                  {formatEventType(event.eventType)}
                </span>
                <span className="shrink-0 rounded-full border border-border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                  {event.actor.actorType}
                </span>
              </div>
              <p className="mt-1 truncate text-[11px] text-muted-foreground">
                {formatTimestamp(event.createdAt)}
                {event.policyResult ? ` · ${formatPolicyResult(event.policyResult)}` : ""}
              </p>
              {(event.sourceRefs.length > 0 || event.evidenceRefs.length > 0) && (
                <p className="mt-1 truncate text-[11px] text-muted-foreground">
                  {formatRefsSummary(event.sourceRefs.length, event.evidenceRefs.length)}
                </p>
              )}
            </div>
          ))}

          {health && !hasActivity && (
            <div className="rounded-md border border-border bg-background px-2.5 py-2 text-xs text-muted-foreground">
              No history yet.
            </div>
          )}
          {!health && (
            <div className="rounded-md border border-border bg-background px-2.5 py-2 text-xs text-muted-foreground">
              Loading history.
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

function formatEventType(eventType: string): string {
  return eventType
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function isHistoryActivityEvent(event: BrainEvent): boolean {
  return Boolean(event.eventType);
}

function formatPolicyResult(policyResult: string): string {
  return policyResult
    .split("_")
    .filter(Boolean)
    .join(" ");
}

function formatRefsSummary(sourceCount: number, evidenceCount: number): string {
  const parts = [];
  if (sourceCount > 0) {
    parts.push(`${sourceCount} source${sourceCount === 1 ? "" : "s"}`);
  }
  if (evidenceCount > 0) {
    parts.push(`${evidenceCount} evidence`);
  }
  return parts.join(" · ");
}

function formatTimestamp(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return "Unknown time";
  }
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
}

export function App() {
  const [snapshot, setSnapshot] = useState<UiSnapshot>(EMPTY_SNAPSHOT);
  const [loadedWorkspaceEnvelope, setLoadedWorkspaceEnvelope] =
    useState<WorkspaceProjectEnvelope | null>(null);
  const [workspaceLoadState, setWorkspaceLoadState] =
    useState<WorkspaceLoadState>({
      status: "idle",
      message: null,
    });
  const [currentConfig, setCurrentConfig] =
    useState<EngineConfigPayload | null>(null);
  const [validation, setValidation] =
    useState<ValidateProviderResponseData | null>(null);
  const [readiness, setReadiness] =
    useState<RuntimeReadinessResponseData | null>(null);
  const [brainHealth, setBrainHealth] =
    useState<BrainHealthResponseData | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileSelection | null>(null);
  const [activePanel, setActivePanel] = useState<ActivePanel>("knowledge");
  const settingsOpen = activePanel === "settings";
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("ai");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(true);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [startupError, setStartupError] = useState<string | null>(null);
  const previewWorkspaceProject = useMemo(
    () => buildWorkspacePreview(snapshot.lastResult, Boolean(snapshot.activeJob)),
    [snapshot.activeJob, snapshot.lastResult],
  );
  const workspaceProject = useMemo(() => {
    if (!loadedWorkspaceEnvelope) {
      return previewWorkspaceProject;
    }

    const envelopeBaseProject =
      loadedWorkspaceEnvelope.project ??
      (loadedWorkspaceEnvelope.sources.length > 0
        ? createEmptyWorkspaceProject(loadedWorkspaceEnvelope.workspace_id)
        : null);
    if (!envelopeBaseProject) {
      return previewWorkspaceProject;
    }

    return hydrateWorkspaceProjectWithSources(
      {
        ...envelopeBaseProject,
        summary: {
          ...envelopeBaseProject.summary,
          stale: envelopeBaseProject.summary.stale || Boolean(snapshot.activeJob),
        },
      },
      loadedWorkspaceEnvelope.sources,
    );
  }, [loadedWorkspaceEnvelope, previewWorkspaceProject, snapshot.activeJob]);
  const graphImportStatus = useMemo(() => {
    if (snapshot.activeJob) {
      return {
        filePath: snapshot.activeJob.filePath,
        format: snapshot.activeJob.format,
        status: snapshot.activeJob.status,
        progressPercent: snapshot.activeJob.progressPercent,
        message: snapshot.activeJob.lastMessage,
      };
    }

    const latestProgress = snapshot.progressLog[0] ?? null;
    if (latestProgress?.phase === "failed") {
      return {
        filePath: selectedFile?.path ?? "Selected file",
        format: selectedFile?.format ?? "document",
        status: "failed",
        progressPercent: 100,
        message: "Import failed",
        failureMessage: latestProgress.message,
        failedPageCount: snapshot.lastResult?.failedCount ?? 0,
      };
    }

    if (snapshot.lastResult && snapshot.lastResult.failedCount > 0) {
      return {
        filePath: selectedFile?.path ?? snapshot.lastResult.savedOutputPath ?? "Imported source",
        format: selectedFile?.format ?? "document",
        status: "partial",
        progressPercent: 100,
        message: "Partial import",
        failedPageCount: snapshot.lastResult.failedCount,
      };
    }

    return null;
  }, [selectedFile, snapshot.activeJob, snapshot.lastResult, snapshot.progressLog]);
  const [workspaceUiState, dispatchWorkspaceUi] = useReducer(
    workspaceUiStateReducer,
    null,
    createInitialWorkspaceUiState,
  );
  useEffect(() => {
    let unlisten: DesktopUnlisten | null = null;

    const bootstrap = async () => {
      const desktop = getDesktopApi();
      const [
        initialSnapshot,
        initialConfig,
        initialValidation,
        initialReadiness,
        initialBrainHealth,
      ] =
        await Promise.all([
          invoke("app_snapshot"),
          invoke("load_engine_config"),
          invoke("validate_engine_config"),
          invoke("engine_readiness"),
          invoke("brain_health"),
        ]);
      setWorkspaceLoadState({
        status: "loading",
        message: "Loading latest materialized graph/wiki snapshot.",
      });
      const initialWorkspaceLoad = await loadGraphWorkspaceEnvelopeResult(
        initialSnapshot.lastWorkspaceId ?? null,
        initialSnapshot.lastProjectId ?? null,
      );
      setSnapshot(initialSnapshot);
      setCurrentConfig(initialConfig);
      setValidation(initialValidation);
      setReadiness(initialReadiness);
      setBrainHealth(initialBrainHealth);
      setLoadedWorkspaceEnvelope(initialWorkspaceLoad.envelope);
      setWorkspaceLoadState(workspaceLoadStateFromResult(initialWorkspaceLoad));

      unlisten = desktop.listen<UiSnapshot>("hyprduck://snapshot", (message) => {
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

    const workspaceProjectId = snapshot.lastWorkspaceId
      ? `workspace:${snapshot.lastWorkspaceId}`
      : null;
    const projectIdToLoad = workspaceProjectId ?? snapshot.lastProjectId ?? null;

    if (projectIdToLoad) {
      setWorkspaceLoadState({
        status: "loading",
        message: "Refreshing from the latest materialized graph/wiki snapshot.",
      });
      loadGraphWorkspaceEnvelopeResult(snapshot.lastWorkspaceId ?? null, projectIdToLoad)
        .then((result) => {
          if (!cancelled) {
            setLoadedWorkspaceEnvelope(result.envelope);
            setWorkspaceLoadState(workspaceLoadStateFromResult(result));
          }
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setLoadedWorkspaceEnvelope(null);
            setWorkspaceLoadState({
              status: "error",
              message: String(error),
            });
          }
        });
      return () => {
        cancelled = true;
      };
    }

    if (snapshot.lastResult) {
      setLoadedWorkspaceEnvelope(null);
      setWorkspaceLoadState({
        status: "idle",
        message: null,
      });
    }

    return () => {
      cancelled = true;
    };
  }, [
    snapshot.lastProjectId,
    snapshot.lastWorkspaceId,
    snapshot.workspaceRevision,
    snapshot.lastResult?.savedOutputPath,
  ]);

  useEffect(() => {
    dispatchWorkspaceUi({
      type: "sync_project",
      project: workspaceProject,
    });
  }, [workspaceProject]);

  const chooseFile = async () => {
    const selection = await invoke("pick_import_file");
    if (selection) {
      setSelectedFile(selection);
      setActivePanel("knowledge");
      try {
        await invoke("start_parse", {
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
    await invoke("start_parse", {
      request: {
        path: selectedFile.path,
        format: selectedFile.format,
      },
    });
    setActivePanel("knowledge");
  };

  const retryFailedPages = async () => {
    await invoke("retry_failed_pages");
    setActivePanel("knowledge");
  };

  const cancelParse = async () => {
    await invoke("cancel_parse");
  };

  const openSavedOutput = async (reveal: boolean) => {
    const path = snapshot.lastResult?.savedOutputPath;
    if (!path) {
      return;
    }
    await invoke("open_saved_output", { path, reveal });
  };

  const openLocalArtifact = async (path: string, reveal: boolean) => {
    await invoke("open_local_artifact", { path, reveal });
  };

  const applyWorkspaceCorrection = async (
    request: WorkspaceApplyCorrectionRequest,
  ) => {
    const appliedProject = await invoke("apply_workspace_correction", {
      correction: {
        projectId: request.projectId,
        nodeId: request.nodeId,
        kind: request.kind,
        targetNodeId: request.targetNodeId ?? null,
        value: request.value ?? null,
      },
    });
    setWorkspaceLoadState({
      status: "loading",
      message: "Refreshing graph/wiki after correction.",
    });
    const nextLoad = await loadGraphWorkspaceEnvelopeResult(
      loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? null,
      appliedProject.summary.projectId,
    );
    setLoadedWorkspaceEnvelope(nextLoad.envelope);
    setWorkspaceLoadState(workspaceLoadStateFromResult(nextLoad));
  };

  const answerWorkspaceProject = async (
    request: WorkspaceAnswerProjectRequest,
  ) => {
    return invoke("answer_workspace_project", {
      request: {
        projectId: request.projectId,
        nodeId: request.nodeId ?? null,
        question: request.question,
      },
    });
  };

  const saveConfig = async (payload: EngineConfigPayload) => {
    const saved = await invoke("save_engine_config", {
      payload,
    });
    const nextValidation = await invoke("validate_engine_config", { payload: saved });
    const nextReadiness = await invoke("engine_readiness");
    setCurrentConfig(saved);
    setValidation(nextValidation);
    setReadiness(nextReadiness);
  };

  const validateConfig = async (payload: EngineConfigPayload | null) => {
    const nextValidation = await invoke("validate_engine_config", { payload });
    setValidation(nextValidation);
  };

  const refreshReadiness = async () => {
    const nextReadiness = await invoke("engine_readiness");
    setReadiness(nextReadiness);
  };

  const refreshBrainHealth = async () => {
    const nextHealth = await invoke("brain_health", {
      workspace_id:
        loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? "default",
    });
    setBrainHealth(nextHealth);
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
              setHistoryOpen(false);
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
              setHistoryOpen(false);
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
              setHistoryOpen(false);
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
        aria-expanded={historyOpen}
        aria-label="History"
        title="History"
        data-electron-no-drag
        onClick={() => {
          setHistoryOpen((open) => !open);
          void refreshBrainHealth();
        }}
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
        <HistoryIcon size={14} />
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
            setHistoryOpen(false);
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
      {historyOpen && (
        <HistoryPanel
          health={brainHealth}
          onRefresh={refreshBrainHealth}
        />
      )}
      {!settingsOpen && <WorkspaceSnapshotStatusBanner state={workspaceLoadState} />}
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
            "flex min-h-0 flex-1 flex-col",
            settingsOpen ? "overflow-y-auto p-6 pt-14" : "overflow-hidden",
          )}
        >
          {settingsOpen ? (
            <SettingsPanel
              config={currentConfig}
              onSave={saveConfig}
              onValidate={validateConfig}
              onRefreshReadiness={refreshReadiness}
              readiness={readiness}
              validation={validation}
              tab={settingsTab}
              onTabChange={setSettingsTab}
            />
          ) : activePanel === "knowledge" ? (
            <WorkspaceErrorBoundary>
              <GraphWorkspace
                dispatch={dispatchWorkspaceUi}
                importStatus={graphImportStatus}
                onApplyCorrection={applyWorkspaceCorrection}
                onAskProject={answerWorkspaceProject}
                onOpenArtifact={openLocalArtifact}
                onOpenImport={chooseFile}
                onRetryFailedPages={retryFailedPages}
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
