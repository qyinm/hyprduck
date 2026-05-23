import { fileNameFromPath } from "@/features/workspace/pathUtils";
import type { WorkspaceProject, WorkspaceSourceSummary } from "@/features/workspace/types";

export function hydrateWorkspaceProjectWithSources(
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

export function createEmptyWorkspaceProject(workspaceId?: string | null): WorkspaceProject {
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
