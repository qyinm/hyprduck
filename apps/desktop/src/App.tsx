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
  Bell,
  CheckCircle2,
  BookOpen,
  ChevronDown,
  ChevronRight,
  FileText,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Save,
  Settings,
  ShieldCheck,
  Sparkles,
  XCircle,
} from "lucide-react";
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
import { Textarea } from "@/components/ui/textarea";
import { GraphWorkspace } from "@/features/workspace/GraphWorkspace";
import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import { fileNameFromPath } from "@/features/workspace/pathUtils";
import {
  createInitialWorkspaceUiState,
  workspaceUiStateReducer,
} from "@/features/workspace/state";
import type {
  WorkspaceApplyCorrectionRequest,
  WorkspaceAnswerProjectRequest,
  WorkspaceProjectEnvelope,
  WorkspaceProject,
  WorkspaceProposeBrainUpdateRequest,
  WorkspaceSourceSummary,
} from "@/features/workspace/types";
import { cn } from "@/lib/utils";

type ActivePanel = "knowledge" | "settings";
type SettingsTab = "general" | "ai";

interface UiSnapshot {
  activeJob: ActiveJobSnapshot | null;
  progressLog: ProgressEntry[];
  lastResult: CompletedResultSnapshot | null;
  lastProjectId?: string | null;
  lastWorkspaceId?: string | null;
  lastSourceId?: string | null;
  lastSourceManifestPath?: string | null;
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

interface RuntimeReadinessCheck {
  id: string;
  label: string;
  ready: boolean;
  required: boolean;
  message: string;
}

interface RuntimeReadinessResponseData {
  ready: boolean;
  provider: string;
  model_id: string;
  checks: RuntimeReadinessCheck[];
}

type BrainProposalKind = "memory" | "claim" | "link" | "observation" | "source_note" | "wiki_page";
type BrainProposalStatus = "pending_review" | "accepted" | "rejected";
type BrainHealthStatus = "clean" | "attention_needed";
type BrainReviewDecision = "accept" | "reject";

interface BrainReviewItem {
  reviewId: string;
  proposalId: string;
  workspaceId: string;
  kind: BrainProposalKind;
  status: BrainProposalStatus;
  title: string;
  body: string;
  proposalPath: string;
  sourceRefs: string[];
  nodeRefs: string[];
  evidenceRefs: string[];
  createdAt: number;
}

interface BrainActor {
  actorType: "system" | "user" | "agent";
  actorId: string;
}

interface BrainEvent {
  eventId: string;
  workspaceId: string;
  eventType: string;
  actor: BrainActor;
  sourceRefs: string[];
  nodeRefs: string[];
  relationRefs: string[];
  evidenceRefs: string[];
  payloadJson: string;
  confidence?: string | null;
  policyResult: string;
  createdAt: number;
}

interface BrainHealthResponseData {
  status: BrainHealthStatus;
  attentionCount: number;
  reviewItems: BrainReviewItem[];
  recentEvents: BrainEvent[];
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
    savedOutputPath: "~/Library/Application Support/HyprDuck/web-preview/sample.md",
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

const WEB_MOCK_NOW_SECONDS = Math.floor(Date.now() / 1000);
const WEB_MOCK_REVIEW_ITEMS: BrainReviewItem[] = [
  {
    reviewId: "proposal-web-claim",
    proposalId: "proposal-web-claim",
    workspaceId: "web-preview",
    kind: "claim",
    status: "pending_review",
    title: "Claim needs source-backed approval",
    body: "Imported PDF notes say HyprDuck should keep agent memory updates auditable before they become trusted graph state.",
    proposalPath:
      "~/Library/Application Support/HyprDuck/web-preview/reviews/proposed-updates/proposal-web-claim.json",
    sourceRefs: ["preview"],
    nodeRefs: ["source:preview"],
    evidenceRefs: ["ev-page-1"],
    createdAt: WEB_MOCK_NOW_SECONDS - 300,
  },
  {
    reviewId: "proposal-web-wiki",
    proposalId: "proposal-web-wiki",
    workspaceId: "web-preview",
    kind: "wiki_page",
    status: "pending_review",
    title: "Wiki save-back needs review",
    body: "Agent-authored wiki pages should show a proposed durable write before HyprDuck saves them into the local brain repo.",
    proposalPath:
      "~/Library/Application Support/HyprDuck/web-preview/reviews/proposed-updates/proposal-web-wiki.json",
    sourceRefs: ["preview"],
    nodeRefs: [],
    evidenceRefs: [],
    createdAt: WEB_MOCK_NOW_SECONDS - 120,
  },
];

let webMockReviewItems = WEB_MOCK_REVIEW_ITEMS.map((item) => ({ ...item }));
let webMockRecentEvents: BrainEvent[] = [
  {
    eventId: "evt-web-review-created",
    workspaceId: "web-preview",
    eventType: "review_created",
    actor: { actorType: "agent", actorId: "web-preview-agent" },
    sourceRefs: ["preview"],
    nodeRefs: ["source:preview"],
    relationRefs: [],
    evidenceRefs: ["ev-page-1"],
    payloadJson: "{}",
    confidence: null,
    policyResult: "needs_review",
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
    issues.push({ message: `${provider.label} requires an API key.` });
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
    status: webMockReviewItems.length > 0 ? "attention_needed" : "clean",
    attentionCount: webMockReviewItems.length,
    reviewItems: webMockReviewItems.map((item) => ({ ...item })),
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

function hydrateWorkspaceProjectWithSources(
  project: WorkspaceProject,
  sources: WorkspaceSourceSummary[],
): WorkspaceProject {
  if (sources.length === 0) {
    return project;
  }

  const nodes = [...project.nodes];
  const edges = [...project.edges];
  const detailsByNodeId = { ...project.detailsByNodeId };
  const answerByNodeId = { ...project.answerByNodeId };
  const existingSourceIds = new Set(
    nodes
      .map((node) => detailsByNodeId[node.id]?.source?.sourceId)
      .filter((sourceId): sourceId is string => Boolean(sourceId)),
  );
  const existingNodeIds = new Set(nodes.map((node) => node.id));
  const relatedCountByNodeId = new Map<string, number>();
  for (const edge of edges) {
    relatedCountByNodeId.set(
      edge.sourceNodeId,
      (relatedCountByNodeId.get(edge.sourceNodeId) ?? 0) + 1,
    );
    relatedCountByNodeId.set(
      edge.targetNodeId,
      (relatedCountByNodeId.get(edge.targetNodeId) ?? 0) + 1,
    );
  }

  for (const [index, source] of sources.entries()) {
    if (existingSourceIds.has(source.source_id)) {
      continue;
    }
    const nodeId = sourceNodeId(source.source_id);
    if (existingNodeIds.has(nodeId)) {
      continue;
    }
    const node = {
      id: nodeId,
      label: fileNameFromPath(source.original_path || source.source_path),
      kind: "source" as const,
      confidence: source.status === "failed" ? 0.18 : 0.72,
      relatedCount: relatedCountByNodeId.get(nodeId) ?? 0,
      evidenceCount: source.success_count,
      position: sourceOnlyNodePosition(index, sources.length),
    };
    nodes.push(node);
    detailsByNodeId[nodeId] = {
      node,
      canonicalName: node.label,
      aliases: ["Workspace source"],
      description:
        "Immutable source registered in the workspace. HyprDuck keeps source artifacts addressable even when no graph links have been extracted yet.",
      evidence: [],
      actions: [],
      source: sourceBackingFromSummary(source),
    };
    answerByNodeId[nodeId] = {
      status: source.status === "failed" ? "blocked" : "low_confidence",
      text: null,
      explanation:
        source.status === "failed"
          ? "This source is present in the workspace, but ingest failed before grounded answers could be built."
          : "This source is present in the workspace. Select linked graph nodes or inspect derived artifacts for grounded evidence.",
      citations: [],
      relatedNodeIds: [],
      suggestedActions: [
        {
          kind: "inspect_evidence",
          label: "Inspect source artifacts",
          description:
            "Open the source detail inspector to review the copied source and raw markdown artifact.",
        },
      ],
    };
    existingSourceIds.add(source.source_id);
    existingNodeIds.add(nodeId);
  }

  return {
    ...project,
    nodes,
    edges,
    detailsByNodeId,
    answerByNodeId,
    summary: {
      ...project.summary,
      nodeCount: nodes.length,
      relationshipCount: edges.length,
      documentCount: sources.length || project.summary.documentCount,
    },
  };
}

function createEmptyWorkspaceProject(workspaceId?: string | null): WorkspaceProject {
  return {
    summary: {
      projectId: workspaceId ? `workspace:${workspaceId}` : "workspace:empty",
      title: "Workspace sources",
      status: "preview",
      stale: false,
      summary: "Source-only workspace view.",
      documentCount: 0,
      nodeCount: 0,
      relationshipCount: 0,
      evidenceCount: 0,
    },
    nodes: [],
    edges: [],
    detailsByNodeId: {},
    edgeDetailsById: {},
    answerByNodeId: {},
  };
}

function sourceBackingFromSummary(source: WorkspaceSourceSummary) {
  return {
    workspaceId: source.workspace_id,
    sourceId: source.source_id,
    originalPath: source.original_path,
    sourcePath: source.source_path,
    markdownPath: source.markdown_path,
    format: source.format,
    status: source.status,
    pageCount: source.page_count,
    successCount: source.success_count,
    failedCount: source.failed_count,
    description: source.description ?? "",
    userContext: source.user_context ?? "",
    ingestInstruction: source.ingest_instruction ?? "",
    updatedAt: source.updated_at,
    manifestPath: null,
  };
}

function sourceNodeId(sourceId: string) {
  return `source:${sourceId}`;
}

function sourceOnlyNodePosition(index: number, total: number) {
  const radius = 34;
  const angle = (index / Math.max(1, total)) * Math.PI * 2 - Math.PI / 2;
  return {
    x: 50 + Math.cos(angle) * radius,
    y: 50 + Math.sin(angle) * radius,
  };
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
        case "engine_readiness": {
          return deriveWebReadiness() as T;
        }
        case "brain_health": {
          return createWebBrainHealth() as T;
        }
        case "resolve_brain_review": {
          const proposalId = String(args.proposal_id ?? "");
          const decision = String(args.decision ?? "reject") as BrainReviewDecision;
          const resolved = webMockReviewItems.find(
            (item) => item.proposalId === proposalId,
          );
          webMockReviewItems = webMockReviewItems.filter(
            (item) => item.proposalId !== proposalId,
          );
          appendWebBrainEvent({
            eventId: `evt-web-resolved-${Date.now()}`,
            workspaceId: resolved?.workspaceId ?? "web-preview",
            eventType: "review_resolved",
            actor: { actorType: "user", actorId: "local-user" },
            sourceRefs: resolved?.sourceRefs ?? [],
            nodeRefs: resolved?.nodeRefs ?? [],
            relationRefs: [],
            evidenceRefs: resolved?.evidenceRefs ?? [],
            payloadJson: JSON.stringify({
              proposalId,
              decision,
              reason: args.reason ?? null,
            }),
            confidence: null,
            policyResult: decision,
            createdAt: Math.floor(Date.now() / 1000),
          });
          return {
            proposal: {
              proposalId,
              status: decision === "accept" ? "accepted" : "rejected",
            },
          } as T;
        }
        case "propose_brain_update": {
          const kind = String(args.kind ?? "memory") as BrainProposalKind;
          const proposalId = `proposal-web-${Date.now()}`;
          const reviewable =
            kind === "claim" || kind === "link" || kind === "wiki_page";
          if (reviewable) {
            webMockReviewItems = [
              {
                reviewId: proposalId,
                proposalId,
                workspaceId: "web-preview",
                kind,
                status: "pending_review",
                title: String(args.title ?? "Untitled proposal"),
                body: String(args.body ?? ""),
                proposalPath: `~/Library/Application Support/HyprDuck/web-preview/reviews/proposed-updates/${proposalId}.json`,
                sourceRefs: (args.source_refs as string[] | undefined) ?? [],
                nodeRefs: (args.node_refs as string[] | undefined) ?? [],
                evidenceRefs: (args.evidence_refs as string[] | undefined) ?? [],
                createdAt: Math.floor(Date.now() / 1000),
              },
              ...webMockReviewItems,
            ];
          }
          appendWebBrainEvent({
            eventId: `evt-web-proposed-${Date.now()}`,
            workspaceId: "web-preview",
            eventType:
              kind === "claim"
                ? "claim_proposed"
                : kind === "link"
                  ? "link_proposed"
                  : kind === "wiki_page"
                    ? "wiki_page_proposed"
                    : "memory_proposed",
            actor: { actorType: "user", actorId: "local-user" },
            sourceRefs: (args.source_refs as string[] | undefined) ?? [],
            nodeRefs: (args.node_refs as string[] | undefined) ?? [],
            relationRefs: [],
            evidenceRefs: (args.evidence_refs as string[] | undefined) ?? [],
            payloadJson: JSON.stringify({ title: args.title, body: args.body }),
            confidence: null,
            policyResult: reviewable ? "needs_review" : "auto_applied",
            createdAt: Math.floor(Date.now() / 1000),
          });
          return {
            proposal: {
              proposalId,
              status: reviewable ? "pending_review" : "accepted",
            },
          } as T;
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
          const answer = request?.nodeId
            ? workspace.project.answerByNodeId[request.nodeId]
            : workspace.project.answerByNodeId["source:preview"];
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
        body: "This keeps data local, but small or OCR-only models can miss tables, conflicts, and evidence links. Run the golden corpus before trusting durable graph writes.",
      };
    }

    return {
      tone: "local",
      title: "Local-first path",
      body: "Good for private parsing and retrieval checks. Keep risky merges and project memory writes on review until the golden corpus is clean.",
    };
  }

  return {
    tone: "hosted",
    title: "Hosted quality path",
    body: "Recommended for high-recall page parsing, structured extraction, and merge review when privacy policy allows hosted inference.",
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
              review, and grounded answer workflows.
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

function TrustConsole(props: {
  health: BrainHealthResponseData | null;
  selectedItem: BrainReviewItem | null;
  selectedReviewId: string | null;
  reviewReason: string;
  decisionPending: BrainReviewDecision | null;
  decisionError: string | null;
  onOpenProposal: (path: string) => Promise<void>;
  onReasonChange: (value: string) => void;
  onRefresh: () => Promise<void>;
  onResolve: (item: BrainReviewItem, decision: BrainReviewDecision) => Promise<void>;
  onSelectReview: (reviewId: string) => void;
}) {
  const {
    health,
    selectedItem,
    selectedReviewId,
    reviewReason,
    decisionPending,
    decisionError,
    onOpenProposal,
    onReasonChange,
    onRefresh,
    onResolve,
    onSelectReview,
  } = props;
  const reviewItems = health?.reviewItems ?? [];
  const recentEvents = health?.recentEvents ?? [];
  const attentionCount = health?.attentionCount ?? 0;
  const statusLabel = health?.status === "attention_needed" ? "Needs review" : "Clean";

  return (
    <section
      aria-label="Trust Console"
      className="fixed bottom-4 right-3 top-12 z-50 flex w-[min(58rem,calc(100vw-1.5rem))] flex-col overflow-hidden rounded-xl border border-border bg-background text-sm shadow-xl"
      data-electron-no-drag
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-b border-border px-4 py-4">
        <div className="flex items-start gap-3">
          <span className="mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-full border border-border bg-secondary text-foreground">
            <ShieldCheck size={16} />
          </span>
          <div>
            <h2 className="text-base font-semibold text-foreground">Trust Console</h2>
            <p className="mt-1 max-w-xl text-xs leading-5 text-muted-foreground">
              Review agent-written memory, claim, link, and wiki proposals before
              they become trusted brain state.
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Badge variant={attentionCount > 0 ? "default" : "secondary"}>
            {statusLabel}
          </Badge>
          <Button
            onClick={() => void onRefresh()}
            size="sm"
            type="button"
            variant="outline"
          >
            Refresh
          </Button>
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[18rem_minmax(0,1fr)]">
        <aside className="flex min-h-0 flex-col border-b border-border bg-secondary/25 md:border-b-0 md:border-r">
          <div className="flex items-center justify-between gap-3 border-b border-border px-3 py-3">
            <span className="text-xs font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              Review Queue
            </span>
            <span className="text-xs text-muted-foreground">{attentionCount} pending</span>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            {reviewItems.map((item) => {
              const active = item.reviewId === selectedReviewId;
              return (
                <button
                  className={cn(
                    "mb-2 w-full rounded-lg border px-3 py-3 text-left transition-colors",
                    active
                      ? "border-foreground bg-background text-foreground"
                      : "border-border bg-background/70 text-foreground hover:bg-background",
                  )}
                  key={item.reviewId}
                  onClick={() => onSelectReview(item.reviewId)}
                  type="button"
                >
                  <div className="flex items-center justify-between gap-3">
                    <span className="truncate text-sm font-medium">{item.title}</span>
                    <span className="shrink-0 rounded-full border border-border px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
                      {formatProposalKind(item.kind)}
                    </span>
                  </div>
                  <p className="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">
                    {item.body}
                  </p>
                  <p className="mt-2 text-[11px] text-muted-foreground">
                    {formatTimestamp(item.createdAt)}
                  </p>
                </button>
              );
            })}

            {health && reviewItems.length === 0 && (
              <div className="rounded-lg border border-border bg-background p-3 text-xs leading-5 text-muted-foreground">
                No pending reviews. Proposed writes that passed policy are already
                evented, and risky writes will appear here.
              </div>
            )}
            {!health && (
              <div className="rounded-lg border border-border bg-background p-3 text-xs leading-5 text-muted-foreground">
                Loading brain health.
              </div>
            )}
          </div>
        </aside>

        <div className="flex min-h-0 flex-col overflow-y-auto">
          {selectedItem ? (
            <div className="grid gap-5 p-4">
              <section className="space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge variant="outline">{formatProposalKind(selectedItem.kind)}</Badge>
                  <Badge variant="secondary">{formatProposalStatus(selectedItem.status)}</Badge>
                  <Badge variant="secondary">{selectedItem.workspaceId}</Badge>
                </div>
                <div>
                  <h3 className="text-lg font-semibold tracking-tight text-foreground">
                    {selectedItem.title}
                  </h3>
                  <p className="mt-2 whitespace-pre-wrap text-sm leading-6 text-muted-foreground">
                    {selectedItem.body}
                  </p>
                </div>
              </section>

              <section className="grid gap-3 rounded-xl border border-border bg-secondary/30 p-3">
                <div className="flex items-center gap-2">
                  <FileText size={14} />
                  <h4 className="text-sm font-semibold">Provenance</h4>
                </div>
                <ReferenceRow label="Sources" refs={selectedItem.sourceRefs} />
                <ReferenceRow label="Nodes" refs={selectedItem.nodeRefs} />
                <ReferenceRow label="Evidence" refs={selectedItem.evidenceRefs} />
                <div className="grid gap-1 border-t border-border pt-3 text-xs">
                  <span className="font-medium text-foreground">Proposal file</span>
                  <button
                    className="truncate text-left text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                    onClick={() => void onOpenProposal(selectedItem.proposalPath)}
                    type="button"
                  >
                    {selectedItem.proposalPath}
                  </button>
                </div>
              </section>

              <section className="grid gap-3 rounded-xl border border-border p-3">
                <div>
                  <h4 className="text-sm font-semibold">Review decision</h4>
                  <p className="mt-1 text-xs leading-5 text-muted-foreground">
                    Accept applies the durable save-back. Reject preserves the
                    proposal record and logs the decision without mutating trusted state.
                  </p>
                </div>
                <Textarea
                  className="min-h-20"
                  onChange={(event) => onReasonChange(event.target.value)}
                  placeholder="Optional review note..."
                  value={reviewReason}
                />
                {decisionError && (
                  <p className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                    {decisionError}
                  </p>
                )}
                <div className="flex flex-wrap justify-end gap-2">
                  <Button
                    disabled={decisionPending !== null}
                    onClick={() => void onResolve(selectedItem, "reject")}
                    size="sm"
                    type="button"
                    variant="outline"
                  >
                    <XCircle size={14} />
                    {decisionPending === "reject" ? "Rejecting..." : "Reject"}
                  </Button>
                  <Button
                    disabled={decisionPending !== null}
                    onClick={() => void onResolve(selectedItem, "accept")}
                    size="sm"
                    type="button"
                  >
                    <CheckCircle2 size={14} />
                    {decisionPending === "accept" ? "Accepting..." : "Accept"}
                  </Button>
                </div>
              </section>
            </div>
          ) : (
            <div className="grid min-h-[18rem] place-items-center p-6 text-center">
              <div>
                <ShieldCheck className="mx-auto text-muted-foreground" size={24} />
                <h3 className="mt-3 text-sm font-semibold text-foreground">
                  No proposal selected
                </h3>
                <p className="mt-1 max-w-sm text-xs leading-5 text-muted-foreground">
                  Agent write proposals and maintenance findings appear here when
                  they need human approval.
                </p>
              </div>
            </div>
          )}

          <section className="border-t border-border p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <h3 className="text-sm font-semibold">Recent brain events</h3>
              <span className="text-xs text-muted-foreground">
                {recentEvents.length} shown
              </span>
            </div>
            <div className="grid gap-2">
              {recentEvents.map((event) => (
                <div
                  className="rounded-lg border border-border bg-secondary/30 px-3 py-2 text-xs leading-5"
                  key={event.eventId}
                >
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="font-medium text-foreground">
                      {formatEventType(event.eventType)}
                    </span>
                    <span className="text-muted-foreground">
                      {event.actor.actorType}:{event.actor.actorId} · {event.policyResult}
                    </span>
                  </div>
                  <div className="mt-1 flex flex-wrap gap-2 text-[11px] text-muted-foreground">
                    <span>{formatTimestamp(event.createdAt)}</span>
                    <span>{event.sourceRefs.length} sources</span>
                    <span>{event.evidenceRefs.length} evidence</span>
                  </div>
                </div>
              ))}
              {recentEvents.length === 0 && (
                <div className="rounded-lg border border-border bg-secondary/30 px-3 py-3 text-xs text-muted-foreground">
                  No recent brain events yet.
                </div>
              )}
            </div>
          </section>
        </div>
      </div>
    </section>
  );
}

function ReferenceRow(props: { label: string; refs: string[] }) {
  return (
    <div className="grid gap-2 text-xs sm:grid-cols-[5rem_minmax(0,1fr)]">
      <span className="font-medium text-muted-foreground">{props.label}</span>
      <div className="flex min-w-0 flex-wrap gap-1.5">
        {props.refs.length > 0 ? (
          props.refs.map((ref) => (
            <span
              className="max-w-full truncate rounded-full border border-border bg-background px-2 py-0.5 text-foreground"
              key={ref}
              title={ref}
            >
              {ref}
            </span>
          ))
        ) : (
          <span className="text-muted-foreground">No refs attached</span>
        )}
      </div>
    </div>
  );
}

function formatProposalKind(kind: BrainProposalKind): string {
  switch (kind) {
    case "claim":
      return "Claim";
    case "link":
      return "Link";
    case "memory":
      return "Memory";
    case "observation":
      return "Observation";
    case "source_note":
      return "Source note";
    case "wiki_page":
      return "Wiki page";
  }
}

function formatProposalStatus(status: BrainProposalStatus): string {
  switch (status) {
    case "accepted":
      return "Accepted";
    case "pending_review":
      return "Pending review";
    case "rejected":
      return "Rejected";
  }
}

function formatEventType(eventType: string): string {
  return eventType
    .split("_")
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
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
  const [healthOpen, setHealthOpen] = useState(false);
  const [selectedReviewId, setSelectedReviewId] = useState<string | null>(null);
  const [reviewReason, setReviewReason] = useState("");
  const [reviewDecisionPending, setReviewDecisionPending] =
    useState<BrainReviewDecision | null>(null);
  const [reviewDecisionError, setReviewDecisionError] = useState<string | null>(
    null,
  );
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
  const [workspaceUiState, dispatchWorkspaceUi] = useReducer(
    workspaceUiStateReducer,
    null,
    createInitialWorkspaceUiState,
  );
  const selectedReviewItem = useMemo(() => {
    const items = brainHealth?.reviewItems ?? [];
    if (items.length === 0) {
      return null;
    }
    return (
      items.find((item) => item.reviewId === selectedReviewId) ?? items[0]
    );
  }, [brainHealth?.reviewItems, selectedReviewId]);

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
          invoke<UiSnapshot>("app_snapshot"),
          invoke<EngineConfigPayload>("load_engine_config"),
          invoke<ValidateProviderResponseData>("validate_engine_config"),
          invoke<RuntimeReadinessResponseData>("engine_readiness"),
          invoke<BrainHealthResponseData>("brain_health"),
        ]);
      const initialWorkspaceEnvelope =
        await invoke<WorkspaceProjectEnvelope>("load_workspace_project");
      setSnapshot(initialSnapshot);
      setCurrentConfig(initialConfig);
      setValidation(initialValidation);
      setReadiness(initialReadiness);
      setBrainHealth(initialBrainHealth);
      setLoadedWorkspaceEnvelope(initialWorkspaceEnvelope);

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

    const workspaceProjectId = snapshot.lastWorkspaceId
      ? `workspace:${snapshot.lastWorkspaceId}`
      : null;
    const projectIdToLoad = workspaceProjectId ?? snapshot.lastProjectId ?? null;

    if (projectIdToLoad) {
      invoke<WorkspaceProjectEnvelope>("load_workspace_project", {
        project_id: projectIdToLoad,
        workspace_id: snapshot.lastWorkspaceId ?? undefined,
      })
        .then((envelope) => {
          if (!cancelled) {
            setLoadedWorkspaceEnvelope(envelope);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setLoadedWorkspaceEnvelope(null);
          }
        });
      return () => {
        cancelled = true;
      };
    }

    if (snapshot.lastResult) {
      setLoadedWorkspaceEnvelope(null);
    }

    return () => {
      cancelled = true;
    };
  }, [
    snapshot.lastProjectId,
    snapshot.lastWorkspaceId,
    snapshot.lastResult?.savedOutputPath,
  ]);

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

  useEffect(() => {
    const items = brainHealth?.reviewItems ?? [];
    if (items.length === 0) {
      if (selectedReviewId !== null) {
        setSelectedReviewId(null);
      }
      return;
    }
    if (!selectedReviewId || !items.some((item) => item.reviewId === selectedReviewId)) {
      setSelectedReviewId(items[0].reviewId);
    }
  }, [brainHealth?.reviewItems, selectedReviewId]);

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

  const openLocalArtifact = async (path: string, reveal: boolean) => {
    await invoke<void>("open_local_artifact", { path, reveal });
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
    setLoadedWorkspaceEnvelope((current) => ({
      project,
      workspace_id: current?.workspace_id ?? snapshot.lastWorkspaceId ?? null,
      sources: current?.sources ?? [],
    }));
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

  const proposeBrainUpdate = async (
    request: WorkspaceProposeBrainUpdateRequest,
  ) => {
    const workspaceId =
      request.workspaceId ??
      loadedWorkspaceEnvelope?.workspace_id ??
      snapshot.lastWorkspaceId ??
      "default";
    await invoke<unknown>("propose_brain_update", {
      workspace_id: workspaceId,
      kind: request.kind,
      title: request.title,
      body: request.body,
      target_node_id: request.targetNodeId ?? null,
      target_source_id: request.targetSourceId ?? null,
      relation_kind: request.relationKind ?? null,
      source_description: request.sourceDescription ?? null,
      source_user_context: request.sourceUserContext ?? null,
      source_ingest_instruction: request.sourceIngestInstruction ?? null,
      source_refs: request.sourceRefs ?? [],
      node_refs: request.nodeRefs ?? [],
      evidence_refs: request.evidenceRefs ?? [],
    });
    const nextEnvelope = await invoke<WorkspaceProjectEnvelope>(
      "load_workspace_project",
      {
        project_id: loadedWorkspaceEnvelope?.project?.summary.projectId ?? null,
        workspace_id:
          loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? undefined,
      },
    );
    setLoadedWorkspaceEnvelope(nextEnvelope);
    const nextHealth = await invoke<BrainHealthResponseData>("brain_health", {
      workspace_id: workspaceId,
    });
    setBrainHealth(nextHealth);
  };

  const saveConfig = async (payload: EngineConfigPayload) => {
    const saved = await invoke<EngineConfigPayload>("save_engine_config", {
      payload,
    });
    const nextValidation = await invoke<ValidateProviderResponseData>(
      "validate_engine_config",
      { payload: saved },
    );
    const nextReadiness = await invoke<RuntimeReadinessResponseData>(
      "engine_readiness",
    );
    setCurrentConfig(saved);
    setValidation(nextValidation);
    setReadiness(nextReadiness);
  };

  const validateConfig = async (payload: EngineConfigPayload | null) => {
    const nextValidation = await invoke<ValidateProviderResponseData>(
      "validate_engine_config",
      { payload },
    );
    setValidation(nextValidation);
  };

  const refreshReadiness = async () => {
    const nextReadiness = await invoke<RuntimeReadinessResponseData>(
      "engine_readiness",
    );
    setReadiness(nextReadiness);
  };

  const refreshBrainHealth = async () => {
    const nextHealth = await invoke<BrainHealthResponseData>("brain_health", {
      workspace_id:
        loadedWorkspaceEnvelope?.workspace_id ?? snapshot.lastWorkspaceId ?? "default",
    });
    setBrainHealth(nextHealth);
  };

  const resolveBrainReview = async (
    item: BrainReviewItem,
    decision: BrainReviewDecision,
  ) => {
    const workspaceId =
      item.workspaceId ??
      loadedWorkspaceEnvelope?.workspace_id ??
      snapshot.lastWorkspaceId ??
      "default";
    setReviewDecisionPending(decision);
    setReviewDecisionError(null);
    try {
      await invoke<unknown>("resolve_brain_review", {
        workspace_id: workspaceId,
        proposal_id: item.proposalId,
        decision,
        reason: reviewReason.trim() || null,
      });
      setReviewReason("");
      const [nextHealth, nextEnvelope] = await Promise.all([
        invoke<BrainHealthResponseData>("brain_health", {
          workspace_id: workspaceId,
        }),
        invoke<WorkspaceProjectEnvelope>("load_workspace_project", {
          project_id: loadedWorkspaceEnvelope?.project?.summary.projectId ?? null,
          workspace_id: workspaceId,
        }),
      ]);
      setBrainHealth(nextHealth);
      setLoadedWorkspaceEnvelope(nextEnvelope);
      setSelectedReviewId(nextHealth.reviewItems[0]?.reviewId ?? null);
    } catch (error) {
      setReviewDecisionError(String(error));
    } finally {
      setReviewDecisionPending(null);
    }
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
        aria-label="Brain health"
        title="Brain health"
        data-electron-no-drag
        onClick={() => {
          setHealthOpen((open) => !open);
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
        <Bell size={14} />
        {brainHealth && brainHealth.attentionCount > 0 && (
          <span className="absolute -right-1 -top-1 grid h-4 min-w-4 place-items-center rounded-full bg-destructive px-1 text-[10px] font-semibold leading-none text-destructive-foreground">
            {brainHealth.attentionCount}
          </span>
        )}
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
        <TrustConsole
          decisionError={reviewDecisionError}
          decisionPending={reviewDecisionPending}
          health={brainHealth}
          onOpenProposal={(path) => openLocalArtifact(path, false)}
          onReasonChange={setReviewReason}
          onRefresh={refreshBrainHealth}
          onResolve={resolveBrainReview}
          onSelectReview={(reviewId) => {
            setSelectedReviewId(reviewId);
            setReviewDecisionError(null);
          }}
          reviewReason={reviewReason}
          selectedItem={selectedReviewItem}
          selectedReviewId={selectedReviewId}
        />
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
                onApplyCorrection={applyWorkspaceCorrection}
                onAskProject={answerWorkspaceProject}
                onOpenArtifact={openLocalArtifact}
                onOpenImport={chooseFile}
                onProposeBrainUpdate={proposeBrainUpdate}
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
