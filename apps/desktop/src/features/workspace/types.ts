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
  description: string;
  userContext: string;
  ingestInstruction: string;
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
  description: string;
  user_context: string;
  ingest_instruction: string;
  updated_at: number;
}

export interface WorkspaceCorrectionAction {
  kind: "merge" | "keep_separate" | "rename" | "split";
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

export interface WorkspaceProposeBrainUpdateRequest {
  workspaceId?: string | null;
  kind: "node" | "memory" | "claim" | "link" | "observation" | "source_note";
  title: string;
  body: string;
  targetNodeId?: string | null;
  targetSourceId?: string | null;
  relationKind?: WorkspaceRelationKind | null;
  sourceDescription?: string | null;
  sourceUserContext?: string | null;
  sourceIngestInstruction?: string | null;
  sourceRefs?: string[];
  nodeRefs?: string[];
  evidenceRefs?: string[];
  proposalPayload?: WorkspaceAgentProposalPayload | null;
}

export type WorkspaceAgentProposalPayload =
  | {
      changeType: "new_node";
      node: {
        label: string;
        kind: WorkspaceNodeSummary["kind"];
        sourcePath: string;
        nodeId?: string | null;
        aliases?: string[];
        sourceRefs?: string[];
        evidenceRefs?: string[];
        reason?: string | null;
      };
    }
  | {
      changeType: "new_claim";
      claim: {
        statement: string;
        sourcePath: string;
        claimId?: string | null;
        topicRefs?: string[];
        sourceRefs?: string[];
        evidenceRefs?: string[];
        reason?: string | null;
      };
    }
  | {
      changeType: "new_edge";
      edge: {
        sourceNodeId: string;
        targetNodeId: string;
        kind: WorkspaceRelationKind;
        label: string;
        sourcePath: string;
        edgeId?: string | null;
        sourceRefs?: string[];
        evidenceRefs?: string[];
        reason?: string | null;
      };
    };

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

export type MaterializedGraphNodeKind =
  | "source"
  | "memory"
  | "wiki_page"
  | "person"
  | "company"
  | "project"
  | "product"
  | "team"
  | "event"
  | "decision"
  | "task"
  | "claim"
  | "topic"
  | "concept";

export type MaterializedGraphRelationKind =
  | "mentions"
  | "supports"
  | "contradicts"
  | "supersedes"
  | "same_as"
  | "works_at"
  | "founded"
  | "invested_in"
  | "advises"
  | "attended"
  | "owns"
  | "responsible_for"
  | "decided"
  | "blocks"
  | "depends_on"
  | "source_of"
  | "derived_from"
  | "related_to";

export interface MaterializedGraphNodeRecord {
  nodeId: string;
  kind: MaterializedGraphNodeKind;
  label: string;
  aliases: string[];
  evidenceIds: string[];
  sourceIds: string[];
  confidence?: number | null;
  updatedAt: number;
}

export interface MaterializedGraphRelationRecord {
  relationId: string;
  kind: MaterializedGraphRelationKind;
  sourceNodeId: string;
  targetNodeId: string;
  label: string;
  evidenceIds: string[];
  confidence?: number | null;
  updatedAt: number;
}

export interface MaterializedWikiPage {
  pageId: string;
  workspaceId: string;
  path: string;
  title: string;
  body: string;
  nodeRefs: string[];
  sourceRefs: string[];
  evidenceRefs: string[];
  updatedAt: number;
}

export interface MaterializedGraphSnapshot {
  snapshotId: string;
  sourceIngestId: string;
  workspaceId: string;
  sourceOfTruthPath: string;
  latestReadableSnapshotPath: string;
  createdAt: number;
  materializedAt: number;
  materializedPaths: string[];
  sourcePaths: string[];
  nodes: MaterializedGraphNodeRecord[];
  edges: MaterializedGraphRelationRecord[];
  claims: unknown[];
  memoryRefs: string[];
  wikiPages: MaterializedWikiPage[];
}
