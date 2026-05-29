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

export interface ActiveJobSnapshot {
  jobId: string;
  filePath: string;
  format: string;
  status: string;
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
  brain_health: {
    args: { workspace_id?: string | null } | undefined;
    result: BrainHealthResponseData;
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
  apply_workspace_correction: {
    args: { correction: WorkspaceCorrectionPayload };
    result: WorkspaceProject;
  };
  answer_workspace_project: {
    args: { request: WorkspaceAnswerPayload };
    result: WorkspaceProject["answerByNodeId"][string];
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

export interface HyprDuckDesktopApi {
  invoke<K extends DesktopCommand>(
    command: K,
    ...args: DesktopCommandParameters<K>
  ): Promise<DesktopCommandResult<K>>;
  listen<T>(
    eventName: string,
    handler: (message: DesktopMessage<T>) => void | Promise<void>,
  ): DesktopUnlisten;
}
