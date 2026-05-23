import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import type {
  MaterializedGraphSnapshot,
  WorkspaceAnswerProjectRequest,
  WorkspaceProjectEnvelope,
} from "@/features/workspace/types";
import type {
  ActiveJobSnapshot,
  BrainEvent,
  BrainHealthResponseData,
  DesktopMessage,
  DesktopUnlisten,
  EngineConfigPayload,
  FileSelection,
  HyprDuckDesktopApi,
  ProviderOption,
  RuntimeReadinessCheck,
  RuntimeReadinessResponseData,
  UiSnapshot,
  ValidateProviderResponseData,
  ValidationIssue,
} from "@/appTypes";

const WEB_MOCK_PROVIDER_OPTIONS: ProviderOption[] = [
  {
    id: "open_router",
    label: "OpenRouter",
    requires_api_key: true,
    supports_base_url: true,
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
  model_options: ["llama3.1", "llava:latest", "qwen2.5vl"],
  prompt_template_options: [
    "General",
    "Tutorial",
    "UI flow",
    "Code",
    "Table",
  ],
};

const WEB_MOCK_SAMPLE_FILE: FileSelection = {
  path: "/tmp/hyprduck-sample.pdf",
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
    savedOutputPath: "~/Library/Application Support/HyprDuck/web-preview/sample.md",
    successCount: 2,
    failedCount: 0,
    markdown: WEB_MOCK_MARKDOWN,
  },
  lastProjectId: "preview:sample",
  workspaceRevision: 0,
};

const WEB_MOCK_PROVIDER_MODELS: Record<string, string[]> = {
  open_router: ["gpt-4o", "claude-3.5-sonnet", "llama-3.1-70b"],
  ollama: ["llama3.1", "llava:latest", "qwen2.5vl"],
};

const WEB_MOCK_NOW_SECONDS = Math.floor(Date.now() / 1000);
let webMockRecentEvents: BrainEvent[] = [
  {
    eventId: "evt-web-source-imported",
    workspaceId: "web-preview",
    eventType: "source_imported",
    actor: { actorType: "system", actorId: "web-preview" },
    sourceRefs: ["preview"],
    nodeRefs: ["source:preview"],
    relationRefs: [],
    evidenceRefs: ["ev-page-1"],
    payloadJson: "{}",
    confidence: null,
    policyResult: "applied",
    createdAt: WEB_MOCK_NOW_SECONDS - 300,
  },
];

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
    issues.push({
      code: "provider_config",
      message: `${provider.label} requires an API key.`,
    });
  }
  return {
    ready: issues.length === 0,
    issues,
  };
}

function deriveWebReadiness(): RuntimeReadinessResponseData {
  const validation = deriveWebValidation(webMockConfig);
  const checks: RuntimeReadinessCheck[] = [
    {
      id: "runtime_process",
      label: "Runtime process",
      ready: false,
      required: true,
      message: "Desktop runtime is not available in web preview mode.",
    },
    {
      id: "config_file",
      label: "Engine config",
      ready: true,
      required: true,
      message: "Preview configuration is loaded in memory.",
    },
    {
      id: "provider_config",
      label: "Provider config",
      ready: validation.ready,
      required: true,
      message: validation.ready
        ? `${webMockConfig.provider} is configured for preview.`
        : validation.issues.map((issue) => issue.message).join(" "),
    },
  ];
  return {
    ready: checks
      .filter((check) => check.required)
      .every((check) => check.ready),
    provider: webMockConfig.provider,
    model_id: webMockConfig.model_id,
    checks,
  };
}

function createWebBrainHealth(): BrainHealthResponseData {
  return {
    status: "clean",
    attentionCount: 0,
    recentEvents: webMockRecentEvents.map((event) => ({ ...event })),
  };
}

function appendWebBrainEvent(event: BrainEvent) {
  webMockRecentEvents = [event, ...webMockRecentEvents].slice(0, 12);
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
): WorkspaceProjectEnvelope {
  if (!snapshot.lastResult) {
    return { project: null, workspace_id: "web-preview", sources: [] };
  }
  const project = buildWorkspacePreview(snapshot.lastResult, Boolean(snapshot.activeJob));
  return {
    project,
    workspace_id: "web-preview",
    sources: project
      ? [
          {
            workspace_id: "web-preview",
            source_id: "preview",
            original_path: snapshot.lastResult.savedOutputPath ?? "web-preview.md",
            source_path: snapshot.lastResult.savedOutputPath ?? "web-preview.md",
            markdown_path: snapshot.lastResult.savedOutputPath ?? "web-preview.md",
            format: "markdown",
            status: snapshot.activeJob ? "ingesting" : "ingested",
            page_count: snapshot.lastResult.successCount + snapshot.lastResult.failedCount,
            success_count: snapshot.lastResult.successCount,
            failed_count: snapshot.lastResult.failedCount,
            description: "",
            user_context: "",
            ingest_instruction: "",
            updated_at: 0,
          },
        ]
      : [],
  };
}

function createWebMaterializedGraphSnapshot(): MaterializedGraphSnapshot {
  return {
    snapshotId: "snapshot-web-preview",
    sourceIngestId: "web-preview",
    workspaceId: "web-preview",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: WEB_MOCK_NOW_SECONDS,
    materializedAt: WEB_MOCK_NOW_SECONDS,
    materializedPaths: [
      "graph/nodes.json",
      "graph/edges.json",
      "wiki/index.md",
      "events/brain_events.jsonl",
    ],
    sourcePaths: [webMockSnapshot.lastResult?.savedOutputPath ?? "web-preview.md"],
    nodes: [
      {
        nodeId: "source:preview",
        kind: "source",
        label: "Web preview source",
        aliases: ["Latest import"],
        evidenceIds: ["ev-page-1"],
        sourceIds: ["preview"],
        confidence: 0.72,
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
      {
        nodeId: "concept-agent-ready-knowledge",
        kind: "concept",
        label: "Agent-ready knowledge",
        aliases: ["Materialized graph"],
        evidenceIds: ["ev-page-1"],
        sourceIds: ["preview"],
        confidence: 0.76,
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
    ],
    edges: [
      {
        relationId: "edge-preview-agent-ready-knowledge",
        kind: "derived_from",
        sourceNodeId: "source:preview",
        targetNodeId: "concept-agent-ready-knowledge",
        label: "Derived from source",
        evidenceIds: ["ev-page-1"],
        confidence: 0.74,
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
    ],
    claims: [],
    memoryRefs: [],
    wikiPages: [
      {
        pageId: "wiki-index",
        workspaceId: "web-preview",
        path: "wiki/index.md",
        title: "Workspace index",
        body: WEB_MOCK_MARKDOWN,
        nodeRefs: ["source:preview", "concept-agent-ready-knowledge"],
        sourceRefs: ["preview"],
        evidenceRefs: ["ev-page-1"],
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
    ],
  };
}

export function createWebMockApi(): HyprDuckDesktopApi {
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
        case "engine_readiness": {
          return deriveWebReadiness() as T;
        }
        case "brain_health": {
          return createWebBrainHealth() as T;
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
          const envelope = getWebWorkspaceFromSnapshot();
          if (
            !envelope.project ||
            (projectId && envelope.project.summary.projectId !== projectId)
          ) {
            return { project: null, workspace_id: envelope.workspace_id, sources: envelope.sources } as T;
          }
          return { ...envelope } as T;
        }
        case "load_materialized_graph_snapshot": {
          return createWebMaterializedGraphSnapshot() as T;
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
              workspaceRevision: (webMockSnapshot.workspaceRevision ?? 0) + 1,
              lastResult: {
                savedOutputPath: `~/Library/Application Support/HyprDuck/web-preview/${new Date()
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
        case "retry_failed_pages": {
          emitWebSnapshot({
            ...webMockSnapshot,
            progressLog: [
              ...webMockSnapshot.progressLog,
              {
                phase: "retry",
                message: "Preview failed-page retry completed.",
                timestamp: new Date().toISOString(),
              },
            ],
          });
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
        case "open_local_artifact": {
          const path = String((args.path as string | undefined) ?? "");
          if (typeof window !== "undefined") {
            window.alert(`Cannot open local artifacts from web preview: ${path}`);
          }
          return undefined as T;
        }
        case "apply_workspace_correction": {
          const workspace = getWebWorkspaceFromSnapshot();
          if (!workspace.project) {
            throw new Error("No workspace available in preview mode.");
          }
          return { ...workspace.project } as T;
        }
        case "answer_workspace_project": {
          const request = args.request as
            | WorkspaceAnswerProjectRequest
            | undefined;
          const workspace = getWebWorkspaceFromSnapshot();
          if (!workspace.project) {
            throw new Error("No workspace available in preview mode.");
          }
          const terms = String(request?.question ?? "")
            .toLowerCase()
            .split(/[^a-z0-9]+/)
            .filter((term) => term.length > 1);
          const answerEntries = Object.entries(workspace.project.answerByNodeId);
          const scoredAnswers = answerEntries
            .map(([nodeId, answer]) => {
              const detail = workspace.project?.detailsByNodeId[nodeId];
              const haystack = [
                detail?.canonicalName,
                detail?.description,
                answer.text,
                answer.explanation,
                ...(answer.citations ?? []).map((citation) => citation.snippet),
              ]
                .filter(Boolean)
                .join(" ")
                .toLowerCase();
              const queryScore = terms.filter((term) => haystack.includes(term)).length;
              const selectedBias = request?.nodeId === nodeId ? 1 : 0;
              return { answer, queryScore, selectedBias };
            })
            .sort(
              (left, right) =>
                right.queryScore - left.queryScore ||
                right.selectedBias - left.selectedBias,
            );
          const answer =
            scoredAnswers.find((entry) => entry.queryScore > 0)?.answer ??
            workspace.project.answerByNodeId["source:preview"] ??
            scoredAnswers[0]?.answer;
          if (!answer) {
            throw new Error("No answer available for this workspace in preview mode.");
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
      if (eventName !== "hyprduck://snapshot") {
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
