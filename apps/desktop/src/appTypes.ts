import type { WorkspaceProjectEnvelope } from "@/features/workspace/types";

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

export interface HyprDuckDesktopApi {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(
    eventName: string,
    handler: (message: DesktopMessage<T>) => void | Promise<void>,
  ): DesktopUnlisten;
}
