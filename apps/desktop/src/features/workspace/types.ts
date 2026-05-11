export type WorkspaceProjectStatus = "preview" | "ready" | "degraded";
export type WorkspaceNodeKind = "source" | "document" | "page" | "concept";
export type WorkspaceRelationKind = "source_document" | "related_to";
export type WorkspaceSourceStatus =
  | "added"
  | "rendering"
  | "ingesting"
  | "ingested"
  | "needs_review"
  | "failed"
  | "stale";
export type WorkspaceAnswerStatus =
  | "grounded"
  | "low_confidence"
  | "blocked"
  | "stale";

export interface WorkspaceNodePosition {
  x: number;
  y: number;
}

export interface WorkspaceNodeSummary {
  id: string;
  label: string;
  kind: WorkspaceNodeKind;
  confidence: number | null;
  relatedCount: number;
  evidenceCount: number;
  position: WorkspaceNodePosition;
}

export interface WorkspaceEvidenceRef {
  id: string;
  pageLabel: string;
  pageIndex?: number | null;
  snippet: string;
  sourcePath?: string | null;
  sourceId?: string | null;
  markdownPath?: string | null;
  imagePath?: string | null;
  provenance?: string | null;
}

export interface WorkspaceSourceBacking {
  workspaceId: string;
  sourceId: string;
  originalPath: string;
  sourcePath: string;
  markdownPath: string;
  format: string;
  status: WorkspaceSourceStatus;
  pageCount: number;
  successCount: number;
  failedCount: number;
  updatedAt: number;
  manifestPath?: string | null;
}

export interface WorkspaceSourceSummary {
  workspace_id: string;
  source_id: string;
  original_path: string;
  source_path: string;
  markdown_path: string;
  format: string;
  status: WorkspaceSourceStatus;
  page_count: number;
  success_count: number;
  failed_count: number;
  updated_at: number;
}

export interface WorkspaceCorrectionAction {
  kind: "merge" | "keep_separate" | "rename";
  label: string;
  disabledReason?: string | null;
}

export interface WorkspaceApplyCorrectionRequest {
  projectId: string;
  nodeId: string;
  kind: WorkspaceCorrectionAction["kind"];
  targetNodeId?: string | null;
  value?: string | null;
}

export interface WorkspaceAnswerProjectRequest {
  projectId: string;
  nodeId?: string | null;
  question: string;
}

export interface WorkspaceNodeDetail {
  node: WorkspaceNodeSummary;
  canonicalName: string;
  aliases: string[];
  description: string;
  evidence: WorkspaceEvidenceRef[];
  actions: WorkspaceCorrectionAction[];
  source?: WorkspaceSourceBacking | null;
}

export interface WorkspaceEdgeSummary {
  id: string;
  sourceNodeId: string;
  targetNodeId: string;
  kind: WorkspaceRelationKind;
  label: string;
  confidence: number | null;
  evidenceCount: number;
}

export interface WorkspaceEdgeDetail {
  edge: WorkspaceEdgeSummary;
  explanation: string;
  evidence: WorkspaceEvidenceRef[];
}

export interface WorkspaceSuggestedAction {
  kind:
    | "inspect_evidence"
    | "apply_correction"
    | "reimport_project"
    | "ask_different_question";
  label: string;
  description: string;
}

export interface WorkspaceAnswerResponse {
  status: WorkspaceAnswerStatus;
  text: string | null;
  explanation: string;
  citations: WorkspaceEvidenceRef[];
  relatedNodeIds: string[];
  suggestedActions: WorkspaceSuggestedAction[];
}

export interface WorkspaceProjectSummary {
  projectId: string;
  title: string;
  status: WorkspaceProjectStatus;
  stale: boolean;
  summary: string;
  documentCount: number;
  nodeCount: number;
  relationshipCount: number;
  evidenceCount: number;
}

export interface WorkspaceProject {
  summary: WorkspaceProjectSummary;
  nodes: WorkspaceNodeSummary[];
  edges: WorkspaceEdgeSummary[];
  detailsByNodeId: Record<string, WorkspaceNodeDetail>;
  edgeDetailsById: Record<string, WorkspaceEdgeDetail>;
  answerByNodeId: Record<string, WorkspaceAnswerResponse>;
}

export interface WorkspaceProjectEnvelope {
  project: WorkspaceProject | null;
  workspace_id?: string | null;
  sources: WorkspaceSourceSummary[];
}
