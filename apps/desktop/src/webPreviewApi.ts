import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import type {
  MaterializedGraphSnapshot,
  WorkspaceAnswerProjectRequest,
  WorkspaceProjectEnvelope,
} from "@/features/workspace/types";
import type {
  ActiveJobSnapshot,
  AgentTerminalListResult,
  AgentTerminalSession,
  BrainEvent,
  BrainHealthResponseData,
  DesktopCommand,
  DesktopCommandArgs,
  DesktopCommandParameters,
  DesktopCommandResult,
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

type WebPreviewCommandHandlers = {
  [K in DesktopCommand]: (
    args: DesktopCommandArgs<K>,
  ) => DesktopCommandResult<K> | Promise<DesktopCommandResult<K>>;
};

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
const WEB_MOCK_AGENT_LIST: AgentTerminalListResult = {
  agents: [
    {
      id: "codex",
      label: "Codex",
      detected: true,
      support: "supported",
      commands: ["codex"],
      command: "codex",
      path: "/usr/local/bin/codex",
      launchArgs: [],
      confidence: "high",
      disabledReason: null,
    },
    {
      id: "claude_code",
      label: "Claude Code",
      detected: false,
      support: "supported",
      commands: ["claude"],
      command: null,
      path: null,
      launchArgs: [],
      confidence: "missing",
      disabledReason: "Claude Code command was not found on PATH.",
    },
    {
      id: "pi_agent",
      label: "Pi Agent",
      detected: false,
      support: "experimental",
      commands: ["pi-agent"],
      command: null,
      path: null,
      launchArgs: [],
      confidence: "missing",
      disabledReason: "Pi Agent command was not found on PATH.",
    },
    {
      id: "hermes",
      label: "Hermes",
      detected: false,
      support: "experimental",
      commands: ["hermes"],
      command: null,
      path: null,
      launchArgs: [],
      confidence: "missing",
      disabledReason: "Hermes command was not found on PATH.",
    },
  ],
  shell: {
    available: false,
    label: null,
    command: null,
    path: null,
    reason: "Web preview cannot host native terminal sessions.",
  },
};
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

  const handlers: WebPreviewCommandHandlers = {
    app_snapshot: () => ({ ...webMockSnapshot }),
    load_engine_config: () => ({
      ...webMockConfig,
      provider_options: [...WEB_MOCK_PROVIDER_OPTIONS],
    }),
    validate_engine_config: (args) => {
      const next = deriveWebValidation(args?.payload ?? null);
      webMockValidation = next;
      return { ...next };
    },
    engine_readiness: () => deriveWebReadiness(),
    brain_health: () => createWebBrainHealth(),
    get_models_for_provider: (args) => {
      const key = args.providerSlug ?? webMockConfig.provider ?? "ollama";
      return [...(WEB_MOCK_PROVIDER_MODELS[key] ?? WEB_MOCK_PROVIDER_MODELS.ollama)];
    },
    load_workspace_project: (args) => {
      const envelope = getWebWorkspaceFromSnapshot();
      if (
        !envelope.project ||
        (args.project_id && envelope.project.summary.projectId !== args.project_id)
      ) {
        return {
          project: null,
          workspace_id: envelope.workspace_id,
          sources: envelope.sources,
        };
      }
      return { ...envelope };
    },
    load_materialized_graph_snapshot: () => createWebMaterializedGraphSnapshot(),
    pick_import_file: () => ({ ...WEB_MOCK_SAMPLE_FILE }),
    start_parse: (args) => {
      const filePath = args.request.path;
      const format = args.request.format;
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
    },
    retry_failed_pages: () => {
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
    },
    cancel_parse: () => {
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
    },
    open_saved_output: (args) => {
      if (typeof window !== "undefined") {
        window.alert(`Cannot open local files from web preview: ${args.path}`);
      }
    },
    open_local_artifact: (args) => {
      if (typeof window !== "undefined") {
        window.alert(`Cannot open local artifacts from web preview: ${args.path}`);
      }
    },
    apply_workspace_correction: () => {
      const workspace = getWebWorkspaceFromSnapshot();
      if (!workspace.project) {
        throw new Error("No workspace available in preview mode.");
      }
      return { ...workspace.project };
    },
    answer_workspace_project: (args) => {
      const workspace = getWebWorkspaceFromSnapshot();
      if (!workspace.project) {
        throw new Error("No workspace available in preview mode.");
      }
      const terms = args.request.question
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
          const selectedBias = args.request.nodeId === nodeId ? 1 : 0;
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
      return { ...answer };
    },
    agent_terminal_list_agents: () => WEB_MOCK_AGENT_LIST,
    agent_terminal_create_session: (args) => {
      const agent =
        args.kind === "shell"
          ? {
              id: "terminal_shell" as const,
              label: "Terminal",
              detected: true,
              support: "supported" as const,
              commands: ["zsh"],
              command: "zsh",
              path: "/bin/zsh",
              launchArgs: ["-l"],
              confidence: "high" as const,
              disabledReason: null,
            }
          : WEB_MOCK_AGENT_LIST.agents.find(
              (candidate) => candidate.id === args.agentId,
            );
      if (!agent?.detected) {
        throw new Error(`${agent?.label ?? args.agentId} is not detected.`);
      }
      const session: AgentTerminalSession = {
        id: `preview-agent-${Date.now()}`,
        agent,
        handoff: {
          mcp: {
            status: "available",
            toolHint: "Use HyprDuck MCP get_context_pack/read_context_pack for cited evidence.",
          },
          workspace: {
            workspaceId: args.workspaceId ?? "web-preview",
            projectId: args.projectId ?? "preview:sample",
            nodeId: args.nodeId ?? null,
            sourceId: "preview",
          },
          context: {
            scope: args.contextScope ?? "workspace",
            requiredBeforeFirstPrompt: true,
            attachInstructions: [
              `Workspace: ${args.workspaceId ?? "web-preview"}`,
              "Ask the agent to call HyprDuck MCP get_context_pack before answering.",
              "Use cited evidence refs and page/source refs from the returned context pack.",
            ],
          },
          disclosure: {
            localPathsRedactedByDefault: true,
            externalAgentOwnsWorkflow: true,
          },
        },
        handoffState: "external_confirmation_required",
        backend: {
          backend: "web-preview",
          status: "unavailable",
          reason: "Web preview cannot host native terminal sessions.",
          fallback: "external_ghostty",
        },
        fallback: {
          type: "external_ghostty",
          label: "External Ghostty",
          available: true,
          agentId: agent.id,
          agentCommand: agent.command,
          attachInstructions: [
            `Open Ghostty and run: ${agent.command}`,
            `Workspace: ${args.workspaceId ?? "web-preview"}`,
            "Ask the agent to call HyprDuck MCP get_context_pack before answering.",
            "Use cited evidence refs and page/source refs from the returned context pack.",
          ],
        },
        status: "fallback_required",
        output: "",
        outputSequence: 0,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };
      return session;
    },
    agent_terminal_snapshot_session: () => {
      throw new Error("Web preview does not persist agent terminal sessions.");
    },
    agent_terminal_write_session: () => ({
      status: "ignored",
      reason: "Web preview does not host native terminal sessions.",
    }),
    agent_terminal_resize_session: () => ({
      status: "ignored",
      reason: "Web preview does not host native terminal sessions.",
    }),
    agent_terminal_kill_session: () => ({
      status: "closed",
      reason: "Web preview does not host native terminal sessions.",
    }),
    save_engine_config: (args) => {
      webMockConfig = {
        ...webMockConfig,
        ...args.payload,
        provider_options: WEB_MOCK_PROVIDER_OPTIONS,
      };
      return { ...webMockConfig };
    },
  };

  return {
    async invoke<K extends DesktopCommand>(
      command: K,
      ...args: DesktopCommandParameters<K>
    ): Promise<DesktopCommandResult<K>> {
      const handler = handlers[command] as (
        args: DesktopCommandArgs<K>,
      ) => DesktopCommandResult<K> | Promise<DesktopCommandResult<K>>;
      return handler(args[0] as DesktopCommandArgs<K>);
    },
    listen<T>(
      eventName: string,
      handler: (message: DesktopMessage<T>) => void | Promise<void>,
    ): DesktopUnlisten {
      if (eventName === "hyprduck://agent-terminal") {
        return () => undefined;
      }
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
