export type WorkspaceProjectStatus = "preview" | "ready" | "degraded";
export type WorkspaceNodeKind = "document" | "page" | "concept";
export type WorkspaceRelationKind = "source_document" | "related_to";
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
  snippet: string;
  sourcePath?: string | null;
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
  correctionActions: WorkspaceCorrectionAction[];
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
