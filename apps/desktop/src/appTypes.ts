import type {
  MaterializedGraphSnapshot,
  WorkspaceProject,
  WorkspaceProjectEnvelope,
} from "@/features/workspace/types";

export interface UiSnapshot {
  activeJob: ActiveJobSnapshot | null;
  progressLog: ProgressEntry[];
  lastResult: CompletedResultSnapshot | null;
  lastProjectId?: string | null;
  lastWorkspaceId?: string | null;
  lastSourceId?: string | null;
  lastSourceManifestPath?: string | null;
  workspaceRevision?: number;
}

export type ImportJobLifecycleStatus =
  | "imported"
  | "parsing"
  | "packaging"
  | "citation_ready"
  | "context_ready"
  | "failed"
  | "cancelled"
  | "partial";

export interface ActiveJobSnapshot {
  jobId: string;
  filePath: string;
  format: string;
  status: ImportJobLifecycleStatus;
  progressPercent: number;
  lastMessage: string | null;
}

export interface ProgressEntry {
  phase: string;
  message: string;
  timestamp: string;
}

export interface CompletedResultSnapshot {
  savedOutputPath: string | null;
  successCount: number;
  failedCount: number;
  markdown: string;
}

export interface FileSelection {
  path: string;
  format: string;
}

export type SourceOriginalPreviewKind = "pdf" | "text" | "unsupported" | "missing";

export interface SourceDetailRequest {
  sourceId: string;
  originalPath: string;
  sourcePath: string;
  markdownPath: string;
  format: string;
}

export interface SourceDetailResult {
  sourceId: string;
  fileName: string;
  format: string;
  originalPath: string;
  sourcePath: string;
  markdownPath: string;
  original: {
    kind: SourceOriginalPreviewKind;
    previewUrl: string | null;
    text: string | null;
    truncated: boolean;
    error: string | null;
  };
  markdown: {
    text: string | null;
    missing: boolean;
    error: string | null;
  };
}

export interface ProviderOption {
  id: string;
  label: string;
  requires_api_key: boolean;
  supports_base_url: boolean;
}

export interface ValidationIssue {
  code: string;
  message: string;
}

export interface EngineConfigPayload {
  provider: string;
  model_id: string;
  api_key: string;
  base_url: string | null;
  prompt_template: string;
  provider_options: ProviderOption[];
  model_options: string[];
  prompt_template_options: string[];
}

export interface ValidateProviderResponseData {
  ready: boolean;
  issues: ValidationIssue[];
}

export interface RuntimeReadinessCheck {
  id: string;
  label: string;
  ready: boolean;
  required: boolean;
  message: string;
}

export interface RuntimeReadinessResponseData {
  ready: boolean;
  provider: string;
  model_id: string;
  checks: RuntimeReadinessCheck[];
}

export type BrainHealthStatus = "clean" | "attention_needed";
export type WorkspaceLoadStatus =
  | "idle"
  | "loading"
  | "ready"
  | "fallback"
  | "error";

export interface WorkspaceLoadState {
  status: WorkspaceLoadStatus;
  message: string | null;
}

export interface WorkspaceLoadResult {
  envelope: WorkspaceProjectEnvelope;
  source: "materialized" | "legacy";
  fallbackReason?: string | null;
}

export interface BrainActor {
  actorType: "system" | "user" | "agent";
  actorId: string;
}

export interface BrainEvent {
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

export interface BrainHealthResponseData {
  status: BrainHealthStatus;
  attentionCount: number;
  recentEvents: BrainEvent[];
}

export interface DesktopMessage<T> {
  payload: T;
}

export type DesktopUnlisten = () => void;

export interface WorkspaceCorrectionPayload {
  projectId: string;
  nodeId: string;
  kind: string;
  targetNodeId: string | null;
  value: string | null;
}

export interface WorkspaceAnswerPayload {
  projectId: string;
  nodeId: string | null;
  question: string;
}

export type AgentChatScopeMode = "auto" | "all_docs" | "selected_source" | "graph_context";
export type AgentChatAnswerMode = "general" | "evidence" | "blocked";
export type AgentChatStreamStatus =
  | "resolving_scope"
  | "retrieving_context"
  | "classifying_question"
  | "connecting_provider"
  | "generating"
  | "validating_citations"
  | "complete";
export type AgentChatMessageRole = "user" | "assistant" | "system";

export interface AgentChatMessage {
  id: string;
  role: AgentChatMessageRole;
  text: string;
  createdAt: number;
}

export interface AgentChatAskPayload {
  schemaVersion: "etyma.agent_chat.v1";
  conversationId: string;
  assistantMessageId?: string | null;
  scope: {
    workspaceId: string;
    rootDir?: string | null;
  };
  mode: AgentChatScopeMode;
  selectedNodeId?: string | null;
  sourceIds: string[];
  question: string;
  history: AgentChatMessage[];
  budget?: number | null;
  persistContextPack: boolean;
}

export interface AgentChatProviderSummary {
  id: string;
  label: string;
  modelId: string;
  hosted: boolean;
}

export interface AgentChatCitation {
  evidenceRef: string;
  sourceId: string;
  page: number;
  region?: string | null;
  span?: string | null;
  quotedText: string;
  parseConfidence: string;
  selectionReason: string;
  contentHash: string;
  evidenceType: string;
}

export interface AgentChatRetrievalTrace {
  strategy: string;
  chunksConsidered: number;
  chunksSelected: number;
  budgetRequested: number;
  budgetUsed: number;
  evidenceTypeTrace?: {
    considered: Record<string, number>;
    selected: Record<string, number>;
  };
}

export interface AgentChatAskResult {
  schemaVersion: "etyma.agent_chat.v1";
  conversationId: string;
  answerMode: AgentChatAnswerMode;
  assistantMessage: AgentChatMessage;
  answer: WorkspaceProject["answerByNodeId"][string];
  contextPackId: string;
  persistedContextPackPath?: string | null;
  citations: AgentChatCitation[];
  retrievalTrace: AgentChatRetrievalTrace;
  provider: AgentChatProviderSummary;
  warnings: string[];
}

export interface AgentChatStartResult {
  requestId: string;
  conversationId: string;
  assistantMessageId: string;
}

export type AgentChatStreamEvent =
  | {
      requestId: string;
      type: "started";
      conversationId: string;
      assistantMessageId: string;
      provider: AgentChatProviderSummary;
      answerMode?: AgentChatAnswerMode | null;
    }
  | {
      requestId: string;
      type: "status";
      status: AgentChatStreamStatus;
      message: string;
    }
  | {
      requestId: string;
      type: "delta";
      text: string;
    }
  | {
      requestId: string;
      type: "citation_update";
      citations: AgentChatCitation[];
    }
  | {
      requestId: string;
      type: "final";
      result: AgentChatAskResult;
    }
  | {
      requestId: string;
      type: "error";
      code: string;
      message: string;
    }
  | {
      requestId: string;
      type: "stopped";
      partialText: string;
    };

export interface AgentTerminalAgent {
  id: "codex" | "claude_code" | "pi_agent" | "hermes";
  label: string;
  detected: boolean;
  support: "supported" | "experimental";
  commands: string[];
  command: string | null;
  path: string | null;
  launchArgs: string[];
  confidence: "high" | "medium" | "missing";
  disabledReason: string | null;
}

export interface AgentTerminalShell {
  id: "terminal_shell";
  label: string;
  detected: boolean;
  support: "supported";
  commands: string[];
  command: string | null;
  path: string | null;
  launchArgs: string[];
  confidence: "high" | "missing";
  disabledReason: string | null;
}

export type AgentTerminalSessionTarget =
  | AgentTerminalAgent
  | AgentTerminalShell;

export interface AgentTerminalListResult {
  agents: AgentTerminalAgent[];
  shell: {
    available: boolean;
    label: string | null;
    command: string | null;
    path: string | null;
    reason: string | null;
  };
}

export interface AgentTerminalContextHandoff {
  mcp: {
    status: string;
    toolHint: string;
  };
  workspace: {
    workspaceId: string;
    projectId: string | null;
    nodeId: string | null;
    sourceId: string | null;
  };
  context: {
    scope: string;
    requiredBeforeFirstPrompt: boolean;
    attachInstructions: string[];
  };
  disclosure: {
    localPathsRedactedByDefault: boolean;
    externalAgentOwnsWorkflow: boolean;
  };
}

export interface AgentTerminalSession {
  id: string;
  backendSessionId?: string;
  agent: AgentTerminalSessionTarget;
  handoff: AgentTerminalContextHandoff;
  handoffState?: "writable" | "blocked" | "external_confirmation_required";
  backend: {
    backend: string;
    status: string;
    reason?: string;
    fallback?: string;
    exitCode?: number | null;
    signal?: string | number | null;
  };
  fallback: {
    type: "external_ghostty";
    label: string;
    available: boolean;
    agentId: AgentTerminalSessionTarget["id"];
    agentCommand: string | null;
    attachInstructions: string[];
  };
  status: "running" | "fallback_required" | "handoff_required" | "closed";
  output?: string;
  outputSequence?: number;
  createdAt: string;
  updatedAt: string;
}

export interface AgentTerminalEvent {
  type: "data" | "exit" | "session_closed";
  session: AgentTerminalSession;
  outputDelta?: string;
}

export interface DesktopCommandMap {
  app_snapshot: {
    args: undefined;
    result: UiSnapshot;
  };
  load_engine_config: {
    args: undefined;
    result: EngineConfigPayload;
  };
  save_engine_config: {
    args: { payload: EngineConfigPayload };
    result: EngineConfigPayload;
  };
  validate_engine_config: {
    args: { payload?: EngineConfigPayload | null } | undefined;
    result: ValidateProviderResponseData;
  };
  engine_readiness: {
    args: undefined;
    result: RuntimeReadinessResponseData;
  };
  get_models_for_provider: {
    args: { providerSlug: string };
    result: string[];
  };
  load_workspace_project: {
    args: { project_id?: string | null; workspace_id?: string | null };
    result: WorkspaceProjectEnvelope;
  };
  load_materialized_graph_snapshot: {
    args: { workspace_id?: string | null };
    result: MaterializedGraphSnapshot;
  };
  pick_import_file: {
    args: undefined;
    result: FileSelection | null;
  };
  start_parse: {
    args: { request: FileSelection };
    result: void;
  };
  retry_failed_pages: {
    args: undefined;
    result: void;
  };
  cancel_parse: {
    args: undefined;
    result: void;
  };
  open_saved_output: {
    args: { path: string; reveal: boolean };
    result: void;
  };
  open_local_artifact: {
    args: { path: string; reveal: boolean };
    result: void;
  };
  read_source_detail: {
    args: SourceDetailRequest;
    result: SourceDetailResult;
  };
  apply_workspace_correction: {
    args: { correction: WorkspaceCorrectionPayload };
    result: WorkspaceProject;
  };
  agent_chat_ask: {
    args: { request: AgentChatAskPayload };
    result: AgentChatAskResult;
  };
  agent_chat_start: {
    args: { request: AgentChatAskPayload };
    result: AgentChatStartResult;
  };
  agent_chat_stop: {
    args: { requestId: string };
    result: { stopped: boolean };
  };
  agent_terminal_list_agents: {
    args: undefined;
    result: AgentTerminalListResult;
  };
  agent_terminal_create_session: {
    args: {
      kind?: "agent" | "shell";
      agentId?: AgentTerminalAgent["id"];
      workspaceId?: string | null;
      projectId?: string | null;
      nodeId?: string | null;
      contextScope?: string;
      cols?: number;
      rows?: number;
    };
    result: AgentTerminalSession;
  };
  agent_terminal_snapshot_session: {
    args: { sessionId: string };
    result: AgentTerminalSession;
  };
  agent_terminal_write_session: {
    args: { sessionId: string; input: string };
    result: { status: string; reason?: string };
  };
  agent_terminal_resize_session: {
    args: { sessionId: string; cols: number; rows: number };
    result: { status: string; reason?: string };
  };
  agent_terminal_kill_session: {
    args: { sessionId: string };
    result: { status: string; reason?: string };
  };
}

export type DesktopCommand = keyof DesktopCommandMap;
export type DesktopCommandArgs<K extends DesktopCommand> =
  DesktopCommandMap[K]["args"];
export type DesktopCommandParameters<K extends DesktopCommand> =
  undefined extends DesktopCommandArgs<K>
    ? [args?: Exclude<DesktopCommandArgs<K>, undefined>]
    : [args: DesktopCommandArgs<K>];
export type DesktopCommandResult<K extends DesktopCommand> =
  DesktopCommandMap[K]["result"];

export interface EtymaDesktopApi {
  invoke<K extends DesktopCommand>(
    command: K,
    ...args: DesktopCommandParameters<K>
  ): Promise<DesktopCommandResult<K>>;
  listen<T>(
    eventName: string,
    handler: (message: DesktopMessage<T>) => void | Promise<void>,
  ): DesktopUnlisten;
}
