import {
  Component,
  type ErrorInfo,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useState,
} from "react";
import {
  ArrowLeft,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Save,
  Settings,
  Sparkles,
} from "lucide-react";
import {
  type AgentTerminalAgent,
  type AgentTerminalEvent,
  type AgentTerminalListResult,
  type AgentTerminalSession,
  type DesktopCommand,
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
  WorkspaceProjectEnvelope,
  WorkspaceProject,
} from "@/features/workspace/types";
import { cn } from "@/lib/utils";
import { SettingsPanel, type SettingsTab } from "@/SettingsPanel";
import { createWebMockApi } from "@/webPreviewApi";
import {
  createEmptyWorkspaceProject,
  hydrateWorkspaceProjectWithSources,
} from "@/workspaceSourceHydration";

type ActivePanel = "knowledge" | "settings";

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

async function listAgentTerminalAgents(): Promise<AgentTerminalListResult> {
  return invoke("agent_terminal_list_agents");
}

function listenAgentTerminalEvents(
  handler: (event: AgentTerminalEvent) => void,
): DesktopUnlisten {
  return getDesktopApi().listen<AgentTerminalEvent>(
    "hyprduck://agent-terminal",
    (message) => handler(message.payload),
  );
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
  const [selectedFile, setSelectedFile] = useState<FileSelection | null>(null);
  const [activePanel, setActivePanel] = useState<ActivePanel>("knowledge");
  const settingsOpen = activePanel === "settings";
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("ai");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(true);
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
      ] =
        await Promise.all([
          invoke("app_snapshot"),
          invoke("load_engine_config"),
          invoke("validate_engine_config"),
          invoke("engine_readiness"),
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

  const retryFailedPages = async () => {
    await invoke("retry_failed_pages");
    setActivePanel("knowledge");
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

  const createAgentTerminalSession = async (args: {
    kind?: "agent" | "shell";
    agentId?: AgentTerminalAgent["id"];
    nodeId: string | null;
  }): Promise<AgentTerminalSession> => {
    return invoke("agent_terminal_create_session", {
      kind: args.kind ?? "agent",
      agentId: args.agentId,
      workspaceId: loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? "default",
      projectId: workspaceProject?.summary.projectId ?? null,
      nodeId: args.nodeId,
      contextScope: "workspace",
    });
  };

  const writeAgentTerminalSession = async (args: {
    sessionId: string;
    input: string;
  }) => {
    return invoke("agent_terminal_write_session", args);
  };

  const resizeAgentTerminalSession = async (args: {
    sessionId: string;
    cols: number;
    rows: number;
  }) => {
    return invoke("agent_terminal_resize_session", args);
  };

  const killAgentTerminalSession = async (args: { sessionId: string }) => {
    return invoke("agent_terminal_kill_session", args);
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

  const refreshReadiness = async () => {
    const nextReadiness = await invoke("engine_readiness");
    setReadiness(nextReadiness);
  };

  const loadProviderModels = useCallback(
    (providerId: string) =>
      invoke("get_models_for_provider", {
        providerSlug: providerId,
      }),
    [],
  );

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
      {!settingsOpen && (
        <Button
          aria-expanded={workspaceUiState.inspectorOpen}
          aria-label={
            workspaceUiState.inspectorOpen
              ? "Hide details"
              : "Show details"
          }
          title={
            workspaceUiState.inspectorOpen
              ? "Hide details"
              : "Show details"
          }
          data-electron-no-drag
          onClick={() => {
            dispatchWorkspaceUi({ type: "toggle_inspector" });
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
              onRefreshReadiness={refreshReadiness}
              onLoadProviderModels={loadProviderModels}
              readiness={readiness}
              validation={validation}
              tab={settingsTab}
            />
          ) : activePanel === "knowledge" ? (
            <WorkspaceErrorBoundary>
              <GraphWorkspace
                dispatch={dispatchWorkspaceUi}
                importStatus={graphImportStatus}
                onApplyCorrection={applyWorkspaceCorrection}
                onCreateAgentTerminalSession={createAgentTerminalSession}
                onKillAgentTerminalSession={killAgentTerminalSession}
                onListenAgentTerminalEvents={listenAgentTerminalEvents}
                onListAgentTerminalAgents={listAgentTerminalAgents}
                onOpenArtifact={openLocalArtifact}
                onOpenImport={chooseFile}
                onResizeAgentTerminalSession={resizeAgentTerminalSession}
                onRetryFailedPages={retryFailedPages}
                onWriteAgentTerminalSession={writeAgentTerminalSession}
                project={workspaceProject}
                uiState={workspaceUiState}
                workspaceId={loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? "default"}
              />
            </WorkspaceErrorBoundary>
          ) : null}
        </div>
      </section>
    </main>
  );
}
