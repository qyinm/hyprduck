export type WorkspaceProjectStatus = "preview" | "ready" | "degraded";
export type WorkspaceNodeKind = "document" | "page" | "concept";
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
  sourceLabel: string;
}

export interface WorkspaceCorrectionAction {
  id: string;
  label: string;
  disabledReason: string;
}

export interface WorkspaceNodeDetail {
  node: WorkspaceNodeSummary;
  canonicalName: string;
  aliases: string[];
  description: string;
  evidence: WorkspaceEvidenceRef[];
  correctionActions: WorkspaceCorrectionAction[];
}

export interface WorkspaceSuggestedAction {
  id: string;
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
  evidenceCount: number;
}

export interface WorkspaceProject {
  summary: WorkspaceProjectSummary;
  nodes: WorkspaceNodeSummary[];
  detailsByNodeId: Record<string, WorkspaceNodeDetail>;
  answerByNodeId: Record<string, WorkspaceAnswerResponse>;
}
