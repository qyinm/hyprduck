import { type ReactNode, useEffect, useState } from "react";
import {
  BarChart3,
  Cpu,
  FileText,
  PanelLeftClose,
  PanelLeftOpen,
  Settings,
  Upload,
  Wrench,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

type ActivePanel = "import" | "settings";
type StatusTone = "" | "good" | "warn" | "bad";
type KpiTone = "default" | "good" | "warn" | "bad";

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

interface StatCardProps {
  icon: ReactNode;
  label: string;
  value: string;
  tone?: KpiTone;
  detail: string;
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

function kpiBorderClass(tone: KpiTone): string {
  switch (tone) {
    case "good":
      return "border-l-emerald-500";
    case "warn":
      return "border-l-amber-500";
    case "bad":
      return "border-l-rose-500";
    default:
      return "border-l-border";
  }
}

function isActiveToneClass(tone: StatusTone): string {
  switch (tone) {
    case "good":
      return "bg-emerald-100 text-emerald-700";
    case "warn":
      return "bg-amber-100/80 text-amber-700";
    case "bad":
      return "bg-rose-100 text-rose-700";
    default:
      return "bg-muted text-muted-foreground";
  }
}

function sidebarButtonClass(active: boolean): string {
  return cn(
    "w-full justify-start gap-3 border border-transparent rounded-lg px-3 py-2 text-sm",
    active
      ? "bg-secondary text-foreground border-border"
      : "text-muted-foreground hover:text-foreground hover:bg-muted hover:border-border",
  );
}

function StatCard(props: StatCardProps) {
  return (
    <Card className={`rounded-xl border-l-4 ${kpiBorderClass(props.tone ?? "default")}`}>
      <CardHeader className="flex flex-row items-start justify-between gap-2 pb-2">
        <div className="space-y-1">
          <CardTitle className="text-sm text-muted-foreground">{props.label}</CardTitle>
          <div className="text-2xl font-semibold tracking-tight">{props.value}</div>
        </div>
        <Badge variant="secondary" className="shrink-0 rounded-full">
          <span className="text-muted-foreground flex items-center gap-2">{props.icon}</span>
        </Badge>
      </CardHeader>
      <CardContent>
        <p className="text-sm text-muted-foreground">{props.detail}</p>
      </CardContent>
    </Card>
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
  const status = formatStatus(snapshot.activeJob?.status ?? "idle");
  const canStart = Boolean(selectedFile) && !snapshot.activeJob;
  const recentEvents = snapshot.progressLog.slice(0, 8);

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Import dashboard</CardTitle>
          <CardDescription>Pick a file, start parsing, and review results in one workspace.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="rounded-lg border border-dashed border-border bg-muted/20 p-6">
            <p className="font-medium">{selectedFile?.path ?? "No file selected"}</p>
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

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Current job</CardTitle>
          </CardHeader>
          <CardContent>
            {snapshot.activeJob ? (
              <div className="space-y-3">
                <p className="font-mono text-sm break-all text-muted-foreground">{snapshot.activeJob.filePath}</p>
                <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-amber-400 to-rose-500"
                    style={{ width: `${Math.max(6, snapshot.activeJob.progressPercent)}%` }}
                  />
                </div>
                <div className="flex flex-wrap gap-2">
                  <Badge
                    variant={status.tone === "good" ? "secondary" : status.tone === "warn" ? "outline" : "outline"}
                  >
                    {status.label}
                  </Badge>
                  <span className="text-sm text-muted-foreground">
                    {snapshot.activeJob.lastMessage ?? "Waiting for engine events..."}
                  </span>
                </div>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">No active parse. Pick a file and start an import.</p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Latest output</CardTitle>
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
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Recent engine events</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2 max-h-64 overflow-auto">
            {recentEvents.length > 0 ? (
              recentEvents.map((entry) => (
                <div key={`${entry.phase}-${entry.timestamp}`} className="rounded-lg border p-3">
                  <p className="text-sm font-medium">{entry.phase}</p>
                  <p className="text-sm text-muted-foreground">{entry.message}</p>
                  <p className="text-xs text-muted-foreground font-mono">{entry.timestamp}</p>
                </div>
              ))
            ) : (
              <p className="text-sm text-muted-foreground">No events yet. Start an import to populate progress.</p>
            )}
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

function SettingsPanel(props: {
  config: EngineConfigPayload | null;
  validation: ValidateProviderResponseData | null;
  onSave: (payload: EngineConfigPayload) => Promise<void>;
  onValidate: (payload: EngineConfigPayload | null) => Promise<void>;
}) {
  const { config, validation, onSave, onValidate } = props;
  const [formState, setFormState] = useState<EngineConfigPayload | null>(config);

  useEffect(() => {
    setFormState(config);
  }, [config]);

  if (!formState) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>AI Settings</CardTitle>
          <CardDescription>Loading engine configuration...</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  const updateField = <K extends keyof EngineConfigPayload>(key: K, value: EngineConfigPayload[K]) => {
    setFormState((current) => (current ? { ...current, [key]: value } : current));
  };

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>AI settings</CardTitle>
          <CardDescription>Configure provider, model, and key usage for parsing.</CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="grid gap-4 md:grid-cols-2"
            onSubmit={(event) => {
              event.preventDefault();
              void onSave(formState);
            }}
          >
            <div className="space-y-2">
              <Label htmlFor="provider-select">Provider</Label>
              <select
                id="provider-select"
                className="h-9 w-full rounded-md border border-input bg-background px-3"
                onChange={(event) => updateField("provider", event.target.value)}
                value={formState.provider}
              >
                {formState.provider_options.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="model-select">Model</Label>
              <select
                id="model-select"
                className="h-9 w-full rounded-md border border-input bg-background px-3"
                onChange={(event) => updateField("model_id", event.target.value)}
                value={formState.model_id}
              >
                {formState.model_options.map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2 md:col-span-2">
              <Label htmlFor="prompt-template-select">Prompt template</Label>
              <select
                id="prompt-template-select"
                className="h-9 w-full rounded-md border border-input bg-background px-3"
                onChange={(event) => updateField("prompt_template", event.target.value)}
                value={formState.prompt_template}
              >
                {formState.prompt_template_options.map((option) => (
                  <option key={option} value={option}>
                    {option}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="api-key-input">API Key</Label>
              <Input
                id="api-key-input"
                autoComplete="off"
                onChange={(event) => updateField("api_key", event.target.value)}
                placeholder="Required for cloud providers"
                type="password"
                value={formState.api_key}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="base-url-input">Base URL</Label>
              <Input
                id="base-url-input"
                onChange={(event) => updateField("base_url", event.target.value || null)}
                placeholder="Optional custom endpoint (Ollama/local etc.)"
                type="text"
                value={formState.base_url ?? ""}
              />
            </div>

            <div className="flex flex-wrap gap-2 pt-2 md:col-span-2">
              <Button type="submit">Save Settings</Button>
              <Button variant="secondary" onClick={() => void onValidate(formState)} type="button">
                Validate
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardContent>
          {validation ? (
            <div
              className={`rounded-lg border p-3 ${
                validation.ready ? "border-emerald-300/60 bg-emerald-50" : "border-amber-300/60 bg-amber-50"
              }`}
            >
              <p className="font-medium">{validation.ready ? "Validation passed" : "Needs attention"}</p>
              <p className="text-sm text-muted-foreground">
                {validation.issues.map((issue) => issue.message).join(" ") || "No issues detected."}
              </p>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">No validation state yet. Save or validate to verify settings.</p>
          )}
        </CardContent>
      </Card>
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

  const status = formatStatus(snapshot.activeJob?.status ?? "idle");
  const providerSummary = currentConfig
    ? `${currentConfig.provider} · ${currentConfig.model_id} · ${currentConfig.prompt_template ?? "General"}`
    : "Loading provider configuration...";
  const totalCompletedPages = snapshot.lastResult ? snapshot.lastResult.successCount + snapshot.lastResult.failedCount : 0;
  const activeFormat = snapshot.activeJob?.format?.toUpperCase() ?? "Not active";

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
              <div className={cn("rounded-xl border border-sidebar-border bg-sidebar/70 p-3", sidebarOpen ? "block" : "hidden")}>
                <div className="mb-2 text-xs text-muted-foreground">Engine status</div>
                <p className="text-sm font-medium">{currentConfig ? currentConfig.provider : "Not set"}</p>
                <p className="line-clamp-2 text-xs text-muted-foreground">{providerSummary}</p>
              </div>

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

              <div className={cn("mt-auto rounded-xl border border-sidebar-border bg-background/70 p-3", sidebarOpen ? "block" : "hidden")}>
                <div className="mb-2 flex items-center gap-2 text-sm">
                  <Wrench aria-hidden="true" size={16} />
                  <span>Workspace status</span>
                </div>
                <p className="line-clamp-3 text-xs text-muted-foreground">
                  Single-window flow with independent sidebar and canvas scrolling.
                </p>
              </div>
            </div>
          </aside>

          <section className="flex min-w-0 flex-col overflow-hidden">
            <header className="sticky top-0 z-10 flex h-16 flex-none items-center justify-between border-b bg-card/80 px-5 backdrop-blur">
              <div>
                <p className="text-xs uppercase tracking-wide text-muted-foreground">Workspace</p>
                <h1 className="text-xl font-semibold">Document parsing</h1>
              </div>
              <div className="flex items-center gap-2">
                <Badge variant="secondary" className={cn("rounded-full", isActiveToneClass(status.tone))}>
                  {status.label}
                </Badge>
              </div>
            </header>

            <div className="min-h-0 flex-1 overflow-y-auto p-5">
              <div className="mb-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                <StatCard
                  detail={providerSummary}
                  icon={<Cpu size={14} />}
                  label="Provider"
                  tone={currentConfig ? "good" : "warn"}
                  value={currentConfig?.provider ?? "Not set"}
                />
                <StatCard
                  detail={status.label}
                  icon={<Upload size={14} />}
                  label="Current job"
                  tone={status.tone || "default"}
                  value={snapshot.activeJob ? activeFormat : "Idle"}
                />
                <StatCard
                  detail={`${totalCompletedPages} pages`}
                  icon={<BarChart3 size={14} />}
                  label="Parsed pages"
                  tone={totalCompletedPages > 0 ? "good" : "default"}
                  value={String(totalCompletedPages)}
                />
                <StatCard
                  detail="Validation state for AI engine"
                  icon={<Settings size={14} />}
                  label="Engine"
                  tone={validation?.ready ? "good" : "warn"}
                  value={validation?.ready ? "Ready" : "Check"}
                />
              </div>

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
