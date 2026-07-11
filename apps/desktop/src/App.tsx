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
  FileText,
  MessageCircle,
  Network,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Settings,
  Sparkles,
} from "lucide-react";
import {
  type AgentChatAskPayload,
  type AgentChatStartResult,
  type AgentChatStreamEvent,
  type DesktopMessage,
  type DesktopUnlisten,
  type EngineConfigPayload,
  type FileSelection,
  type SourceDetailResult,
  type UiSnapshot,
  type ValidateProviderResponseData,
  type RuntimeReadinessResponseData,
  type WorkspaceLoadState,
} from "@/appTypes";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { getDesktopApi, invoke } from "@/desktopApi";
import { AgentChatWorkspace } from "@/features/agent-chat/AgentChatWorkspace";
import { DocsWorkspace } from "@/features/workspace/DocsWorkspace";
import { GraphWorkspace } from "@/features/workspace/GraphWorkspace";
import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import {
  loadGraphWorkspaceEnvelopeResult,
  workspaceLoadStateFromResult,
} from "@/features/workspace/loadWorkspaceEnvelope";
import {
  createInitialWorkspaceUiState,
  workspaceUiStateReducer,
} from "@/features/workspace/state";
import type {
  WorkspaceApplyCorrectionRequest,
  WorkspaceProjectEnvelope,
  WorkspaceSourceSummary,
} from "@/features/workspace/types";
import { useI18n } from "@/i18n/I18nProvider";
import type { TranslationKey } from "@/i18n/locales";
import { cn } from "@/lib/utils";
import { SettingsPanel, type SettingsTab } from "@/SettingsPanel";
import {
  createEmptyWorkspaceProject,
  hydrateWorkspaceProjectWithSources,
} from "@/workspaceSourceHydration";

type MainPanel = "docs" | "agent" | "graph";
type ActivePanel = MainPanel | "settings";

const MAIN_NAV_ITEMS: { id: MainPanel; labelKey: TranslationKey; icon: ReactNode }[] = [
  {
    id: "docs",
    labelKey: "nav.docs",
    icon: <FileText aria-hidden="true" size={18} />,
  },
  {
    id: "agent",
    labelKey: "nav.agent",
    icon: <MessageCircle aria-hidden="true" size={18} />,
  },
  {
    id: "graph",
    labelKey: "nav.graph",
    icon: <Network aria-hidden="true" size={18} />,
  },
];

const SETTINGS_NAV_ITEMS: {
  id: SettingsTab;
  labelKey: TranslationKey;
  icon: ReactNode;
}[] = [
  {
    id: "general",
    labelKey: "nav.general",
    icon: <Settings aria-hidden="true" size={18} />,
  },
  { id: "ai", labelKey: "nav.ai", icon: <Sparkles aria-hidden="true" size={18} /> },
];

const EMPTY_SNAPSHOT: UiSnapshot = {
  activeJob: null,
  progressLog: [],
  lastResult: null,
  lastProjectId: null,
  workspaceRevision: 0,
};

class WorkspaceErrorBoundary extends Component<
  {
    children: ReactNode;
    renderCopy: {
      unknownError: string;
      title: string;
      body: string;
    };
  },
  { errorMessage: string | null; unknown: boolean }
> {
  constructor(props: {
    children: ReactNode;
    renderCopy: {
      unknownError: string;
      title: string;
      body: string;
    };
  }) {
    super(props);
    this.state = { errorMessage: null, unknown: false };
  }

  static getDerivedStateFromError(error: unknown) {
    return {
      errorMessage: error instanceof Error ? error.message : null,
      unknown: !(error instanceof Error),
    };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("Workspace render failed", error, info);
  }

  render() {
    if (this.state.errorMessage || this.state.unknown) {
      return (
        <div className="flex min-h-[24rem] flex-col items-center justify-center rounded-[24px] border border-red-200 bg-red-50/80 p-8 text-center">
          <h2 className="text-lg font-semibold text-red-900">
            {this.props.renderCopy.title}
          </h2>
          <p className="mt-3 max-w-2xl text-sm leading-6 text-red-800">
            {this.props.renderCopy.body}
          </p>
          <pre className="mt-4 max-w-3xl overflow-x-auto rounded-2xl bg-white/90 px-4 py-3 text-left text-xs leading-6 text-red-900">
            {this.state.errorMessage ?? this.props.renderCopy.unknownError}
          </pre>
        </div>
      );
    }

    return this.props.children;
  }
}

function WorkspaceSnapshotStatusBanner({
  state,
  t,
}: {
  state: WorkspaceLoadState;
  t: ReturnType<typeof useI18n>["t"];
}) {
  if (state.status === "idle" || state.status === "ready") {
    return null;
  }

  const title =
    state.status === "loading"
      ? t("workspace.status.refreshingTitle")
      : state.status === "fallback"
        ? t("workspace.status.fallbackTitle")
        : t("workspace.status.errorTitle");
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
  const { t } = useI18n();
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
  const [activePanel, setActivePanel] = useState<ActivePanel>("docs");
  const settingsOpen = activePanel === "settings";
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
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
        filePath: selectedFile?.path ?? t("workspace.import.selectedFile"),
        format: selectedFile?.format ?? "document",
        status: "failed",
        progressPercent: 100,
        message: t("workspace.import.failed"),
        failureMessage: latestProgress.message,
        failedPageCount: snapshot.lastResult?.failedCount ?? 0,
      };
    }

    if (snapshot.lastResult && snapshot.lastResult.failedCount > 0) {
      return {
        filePath: selectedFile?.path ?? snapshot.lastResult.savedOutputPath ?? t("workspace.import.importedSource"),
        format: selectedFile?.format ?? "document",
        status: "partial",
        progressPercent: 100,
        message: t("workspace.import.partial"),
        failedPageCount: snapshot.lastResult.failedCount,
      };
    }

    return null;
  }, [selectedFile, snapshot.activeJob, snapshot.lastResult, snapshot.progressLog, t]);
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
        message: t("workspace.status.loadingInitial"),
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
      setWorkspaceLoadState(workspaceLoadStateFromResult(initialWorkspaceLoad, t));

      unlisten = desktop.listen<UiSnapshot>("etyma://snapshot", (message) => {
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
        message: t("workspace.status.refreshingInitial"),
      });
      loadGraphWorkspaceEnvelopeResult(snapshot.lastWorkspaceId ?? null, projectIdToLoad)
        .then((result) => {
          if (!cancelled) {
            setLoadedWorkspaceEnvelope(result.envelope);
            setWorkspaceLoadState(workspaceLoadStateFromResult(result, t));
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
      setActivePanel("docs");
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
    setActivePanel("docs");
  };

  const openLocalArtifact = async (path: string, reveal: boolean) => {
    await invoke("open_local_artifact", { path, reveal });
  };

  const readSourceDetail = async (
    source: WorkspaceSourceSummary,
  ): Promise<SourceDetailResult> =>
    invoke("read_source_detail", {
      sourceId: source.source_id,
      originalPath: source.original_path,
      sourcePath: source.source_path,
      markdownPath: source.markdown_path,
      format: source.format,
    });

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
      message: t("workspace.status.refreshingAfterCorrection"),
    });
    const nextLoad = await loadGraphWorkspaceEnvelopeResult(
      loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? null,
      appliedProject.summary.projectId,
    );
    setLoadedWorkspaceEnvelope(nextLoad.envelope);
    setWorkspaceLoadState(workspaceLoadStateFromResult(nextLoad, t));
  };

  const startAgentChat = async (
    request: AgentChatAskPayload,
  ): Promise<AgentChatStartResult> => {
    return invoke("agent_chat_start", { request });
  };

  const stopAgentChat = async (requestId: string): Promise<{ stopped: boolean }> => {
    return invoke("agent_chat_stop", { requestId });
  };

  const listenAgentChatEvents = useCallback(
    (
      handler: (
        message: DesktopMessage<AgentChatStreamEvent>,
      ) => void | Promise<void>,
    ): DesktopUnlisten => getDesktopApi().listen("etyma://agent-chat", handler),
    [],
  );

  const viewSourceInGraph = (sourceId: string) => {
    const sourceNodeId = Object.values(workspaceProject?.detailsByNodeId ?? {}).find(
      (detail) => detail.source?.sourceId === sourceId,
    )?.node.id;
    setActivePanel("graph");
    if (sourceNodeId) {
      dispatchWorkspaceUi({ type: "select_node", nodeId: sourceNodeId });
    }
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
            <CardTitle>{t("app.startup.title")}</CardTitle>
            <CardDescription>{t("app.startup.description")}</CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">{startupError}</p>
          </CardContent>
        </Card>
      </main>
    );
  }

  const showSidebar = !sidebarCollapsed;
  const workspaceSources = loadedWorkspaceEnvelope?.sources ?? [];
  const agentReady = Boolean(validation?.ready && readiness?.ready);

  function openSettings() {
    setActivePanel("settings");
    setSidebarCollapsed(false);
  }

  function closeSettings() {
    setActivePanel("docs");
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
            aria-label="Back to Docs"
            onClick={() => {
              setActivePanel("docs");
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
            aria-label={t("chrome.collapseSidebar")}
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
            aria-label={t("chrome.expandSidebar")}
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
      {activePanel === "graph" && (
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
      {!settingsOpen && <WorkspaceSnapshotStatusBanner state={workspaceLoadState} t={t} />}
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
                      <span className="font-medium">{t(item.labelKey)}</span>
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
                      <span className="font-medium">{t(item.labelKey)}</span>
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
                  <span className="font-medium">{t("nav.settings")}</span>
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
          ) : activePanel === "docs" ? (
            <DocsWorkspace
              importStatus={graphImportStatus}
              onChooseFile={chooseFile}
              onOpenArtifact={openLocalArtifact}
              onReadSourceDetail={readSourceDetail}
              onRetryFailedPages={retryFailedPages}
              onViewInGraph={viewSourceInGraph}
              sources={workspaceSources}
            />
          ) : activePanel === "agent" ? (
            <AgentChatWorkspace
              onListenAgentChatEvents={listenAgentChatEvents}
              onOpenDocs={() => setActivePanel("docs")}
              onStartAgentChat={startAgentChat}
              onStopAgentChat={stopAgentChat}
              project={workspaceProject}
              providerReady={agentReady}
              sources={workspaceSources}
              workspaceId={loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? "default"}
            />
          ) : activePanel === "graph" ? (
            <WorkspaceErrorBoundary
              renderCopy={{
                unknownError: t("workspace.error.unknownRender"),
                title: t("workspace.error.renderTitle"),
                body: t("workspace.error.renderBody"),
              }}
            >
              <GraphWorkspace
                dispatch={dispatchWorkspaceUi}
                importStatus={graphImportStatus}
                onApplyCorrection={applyWorkspaceCorrection}
                onOpenArtifact={openLocalArtifact}
                onOpenDocs={() => setActivePanel("docs")}
                onRetryFailedPages={retryFailedPages}
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
