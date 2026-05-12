import type {
  MaterializedGraphNodeRecord,
  MaterializedGraphRelationRecord,
  MaterializedGraphSnapshot,
  WorkspaceAnswerResponse,
  WorkspaceEdgeDetail,
  WorkspaceEdgeSummary,
  WorkspaceEvidenceRef,
  WorkspaceNodeDetail,
  WorkspaceNodeKind,
  WorkspaceNodeSummary,
  WorkspaceProjectEnvelope,
  WorkspaceRelationKind,
  WorkspaceSourceSummary,
} from "./types";

export function materializedGraphSnapshotToWorkspaceEnvelope(
  snapshot: MaterializedGraphSnapshot,
): WorkspaceProjectEnvelope {
  const visibleMaterializedNodes = snapshot.nodes.filter(
    (node) => !isDerivedMarkdownSourceNode(snapshot, node),
  );
  const visibleNodeIds = new Set(visibleMaterializedNodes.map((node) => node.nodeId));
  const visibleEdges = snapshot.edges.filter(
    (edge) => visibleNodeIds.has(edge.sourceNodeId) && visibleNodeIds.has(edge.targetNodeId),
  );
  const visibleSourcePaths = visibleGraphSourcePaths(snapshot.sourcePaths);
  const nodePositions = layoutNodePositions(visibleMaterializedNodes.length);
  const nodes = visibleMaterializedNodes.map((node, index) =>
    materializedNodeToWorkspaceNode(node, nodePositions[index] ?? { x: 50, y: 50 }),
  );
  const evidenceById = buildEvidenceById(snapshot);
  const relatedCounts = buildRelatedCounts(visibleEdges);
  const detailsByNodeId: Record<string, WorkspaceNodeDetail> = {};
  const answerByNodeId: Record<string, WorkspaceAnswerResponse> = {};

  for (const node of nodes) {
    const materialized = snapshot.nodes.find((entry) => entry.nodeId === node.id);
    if (!materialized) {
      continue;
    }
    const evidence = materialized.evidenceIds.map((id) => evidenceById[id]).filter(Boolean);
    const sourcePath =
      materialized.sourceIds
        .map((sourceId) => sourcePathForSourceId(snapshot, sourceId))
        .find(Boolean) ?? null;
    const markdownPath =
      materialized.sourceIds
        .map((sourceId) => markdownPathForSourceId(snapshot, sourceId))
        .find(Boolean) ?? null;
    const hydratedNode = {
      ...node,
      relatedCount: relatedCounts[node.id] ?? 0,
      evidenceCount: materialized.evidenceIds.length,
    };
    detailsByNodeId[node.id] = {
      node: hydratedNode,
      canonicalName: materialized.label,
      aliases: materialized.aliases,
      description: nodeDescription(materialized, snapshot),
      evidence,
      actions: [],
      source:
        hydratedNode.kind === "source"
          ? sourceBackingForNode(snapshot, materialized, sourcePath, markdownPath)
          : null,
    };
    answerByNodeId[node.id] = {
      status: evidence.length > 0 ? "grounded" : "low_confidence",
      text:
        evidence.length > 0
          ? `${materialized.label} is present in the latest materialized graph snapshot.`
          : null,
      explanation:
        "This graph view is loaded from the latest materialized graph/wiki snapshot, with events JSONL remaining the source of truth.",
      citations: evidence.slice(0, 3),
      relatedNodeIds: relatedNodeIdsForNode(visibleEdges, node.id),
      suggestedActions: [
        {
          kind: "inspect_evidence",
          label: "Inspect evidence",
          description: "Review the materialized evidence and wiki paths linked to this node.",
        },
      ],
    };
  }

  const edgeDetailsById: Record<string, WorkspaceEdgeDetail> = {};
  const edges = visibleEdges.map((edge) => {
    const summary = materializedEdgeToWorkspaceEdge(edge);
    const evidence = edge.evidenceIds.map((id) => evidenceById[id]).filter(Boolean);
    edgeDetailsById[summary.id] = {
      edge: summary,
      explanation: `${edge.label || edge.kind} is loaded from graph/edges.json in snapshot ${snapshot.snapshotId}.`,
      evidence,
    };
    return summary;
  });

  const projectNodes = nodes.map((node) => detailsByNodeId[node.id]?.node ?? node);
  return {
    project: {
      summary: {
        projectId: `workspace:${snapshot.workspaceId}`,
        title: "Workspace knowledge",
        status: projectNodes.length > 0 ? "ready" : "degraded",
        stale: false,
        summary: `Loaded ${projectNodes.length} nodes and ${edges.length} relationships from ${snapshot.latestReadableSnapshotPath}.`,
        documentCount: visibleSourcePaths.length,
        nodeCount: projectNodes.length,
        relationshipCount: edges.length,
        evidenceCount: Object.keys(evidenceById).length,
      },
      nodes: projectNodes,
      edges,
      detailsByNodeId,
      edgeDetailsById,
      answerByNodeId,
    },
    workspace_id: snapshot.workspaceId,
    sources: visibleSourcePaths.map((sourcePath, index) =>
      sourceSummaryFromPath(snapshot, sourcePath, index),
    ),
  };
}

function materializedNodeToWorkspaceNode(
  node: MaterializedGraphNodeRecord,
  position: { x: number; y: number },
): WorkspaceNodeSummary {
  return {
    id: node.nodeId,
    label: node.label,
    kind: workspaceNodeKind(node.kind),
    confidence: node.confidence ?? null,
    relatedCount: 0,
    evidenceCount: node.evidenceIds.length,
    position,
  };
}

function materializedEdgeToWorkspaceEdge(
  edge: MaterializedGraphRelationRecord,
): WorkspaceEdgeSummary {
  return {
    id: edge.relationId,
    sourceNodeId: edge.sourceNodeId,
    targetNodeId: edge.targetNodeId,
    kind: workspaceRelationKind(edge.kind),
    label: edge.label || edge.kind.replace(/_/g, " "),
    confidence: edge.confidence ?? null,
    evidenceCount: edge.evidenceIds.length,
  };
}

function workspaceNodeKind(kind: MaterializedGraphNodeRecord["kind"]): WorkspaceNodeKind {
  if (kind === "source") {
    return "source";
  }
  if (kind === "wiki_page") {
    return "document";
  }
  return "concept";
}

function workspaceRelationKind(
  kind: MaterializedGraphRelationRecord["kind"],
): WorkspaceRelationKind {
  return kind === "source_of" || kind === "derived_from"
    ? "source_document"
    : "related_to";
}

function layoutNodePositions(count: number) {
  const total = Math.max(count, 1);
  return Array.from({ length: count }, (_, index) => {
    const angle = (Math.PI * 2 * index) / total;
    const radius = count <= 2 ? 22 : 34;
    return {
      x: 50 + Math.cos(angle) * radius,
      y: 50 + Math.sin(angle) * radius,
    };
  });
}

function buildEvidenceById(snapshot: MaterializedGraphSnapshot) {
  const evidenceById: Record<string, WorkspaceEvidenceRef> = {};
  for (const page of snapshot.wikiPages) {
    for (const evidenceId of page.evidenceRefs) {
      evidenceById[evidenceId] = {
        id: evidenceId,
        pageLabel: page.title,
        snippet: excerpt(page.body),
        sourcePath: page.sourceRefs
          .map((sourceId) => sourcePathForSourceId(snapshot, sourceId))
          .find(Boolean),
        provenance: page.path,
      };
    }
  }
  for (const node of snapshot.nodes) {
    for (const evidenceId of node.evidenceIds) {
      evidenceById[evidenceId] ??= {
        id: evidenceId,
        pageLabel: "Materialized graph",
        snippet: `Evidence ${evidenceId} is referenced by ${node.label}.`,
        sourcePath: node.sourceIds
          .map((sourceId) => sourcePathForSourceId(snapshot, sourceId))
          .find(Boolean),
        sourceId: node.sourceIds[0] ?? null,
      };
    }
  }
  for (const edge of snapshot.edges) {
    for (const evidenceId of edge.evidenceIds) {
      evidenceById[evidenceId] ??= {
        id: evidenceId,
        pageLabel: "Materialized graph",
        snippet: `Evidence ${evidenceId} supports ${edge.label || edge.kind}.`,
      };
    }
  }
  return evidenceById;
}

function sourcePathForSourceId(snapshot: MaterializedGraphSnapshot, sourceId: string) {
  return (
    snapshot.sourcePaths.find(
      (sourcePath) => sourcePath.includes(sourceId) && !isMarkdownPath(sourcePath),
    ) ??
    snapshot.sourcePaths.find((sourcePath) => sourcePath.includes(sourceId)) ??
    null
  );
}

function markdownPathForSourceId(snapshot: MaterializedGraphSnapshot, sourceId: string) {
  return (
    snapshot.sourcePaths.find(
      (sourcePath) => sourcePath.includes(sourceId) && isMarkdownPath(sourcePath),
    ) ?? null
  );
}

function sourceBackingForNode(
  snapshot: MaterializedGraphSnapshot,
  node: MaterializedGraphNodeRecord,
  sourcePath: string | null,
  markdownPath: string | null,
) {
  return {
    workspaceId: snapshot.workspaceId,
    sourceId: node.sourceIds[0] ?? node.nodeId,
    originalPath: sourcePath ?? node.label,
    sourcePath: sourcePath ?? node.label,
    markdownPath: markdownPath ?? sourcePath ?? node.label,
    format: documentFormatFromPath(sourcePath ?? node.label),
    status: "ingested" as const,
    pageCount: 0,
    successCount: 0,
    failedCount: 0,
    description: "Materialized source node",
    userContext: "",
    ingestInstruction: "",
    updatedAt: node.updatedAt,
    manifestPath: null,
  };
}

function sourceSummaryFromPath(
  snapshot: MaterializedGraphSnapshot,
  sourcePath: string,
  index: number,
): WorkspaceSourceSummary {
  return {
    workspace_id: snapshot.workspaceId,
    source_id: `source-${index + 1}`,
    original_path: sourcePath,
    source_path: sourcePath,
    markdown_path: markdownPathForSourcePath(snapshot, sourcePath) ?? sourcePath,
    format: documentFormatFromPath(sourcePath),
    status: "ingested",
    page_count: 0,
    success_count: 0,
    failed_count: 0,
    description: "",
    user_context: "",
    ingest_instruction: "",
    updated_at: snapshot.materializedAt,
  };
}

function visibleGraphSourcePaths(sourcePaths: string[]) {
  return sourcePaths.filter((sourcePath) => !isDerivedMarkdownPath(sourcePath, sourcePaths));
}

function isDerivedMarkdownSourceNode(
  snapshot: MaterializedGraphSnapshot,
  node: MaterializedGraphNodeRecord,
) {
  if (node.kind !== "source" || !isMarkdownPath(node.label)) {
    return false;
  }
  return node.sourceIds.some((sourceId) =>
    snapshot.sourcePaths.some(
      (sourcePath) => sourcePath.includes(sourceId) && !isMarkdownPath(sourcePath),
    ),
  );
}

function markdownPathForSourcePath(snapshot: MaterializedGraphSnapshot, sourcePath: string) {
  const sourceKey = sourceArtifactKey(sourcePath);
  if (!sourceKey) {
    return null;
  }
  return (
    snapshot.sourcePaths.find(
      (candidate) =>
        candidate !== sourcePath &&
        isMarkdownPath(candidate) &&
        sourceArtifactKey(candidate) === sourceKey,
    ) ?? null
  );
}

function isDerivedMarkdownPath(path: string, allPaths: string[]) {
  if (!isMarkdownPath(path)) {
    return false;
  }
  const sourceKey = sourceArtifactKey(path);
  return (
    path.includes("/artifacts/") ||
    (sourceKey
      ? allPaths.some(
          (candidate) =>
            candidate !== path &&
            !isMarkdownPath(candidate) &&
            sourceArtifactKey(candidate) === sourceKey,
        )
      : false)
  );
}

function sourceArtifactKey(path: string) {
  return path.match(/(?:^|\/)(source-[^/]+)/)?.[1] ?? null;
}

function fileNameFromPath(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function isMarkdownPath(path: string) {
  return fileNameFromPath(path).toLowerCase().endsWith(".md");
}

function documentFormatFromPath(path: string) {
  const name = fileNameFromPath(path).toLowerCase();
  if (name.endsWith(".pdf")) {
    return "pdf";
  }
  if (name.endsWith(".docx")) {
    return "docx";
  }
  if (name.endsWith(".doc")) {
    return "doc";
  }
  if (name.endsWith(".md")) {
    return "markdown";
  }
  return "source";
}

function nodeDescription(
  node: MaterializedGraphNodeRecord,
  snapshot: MaterializedGraphSnapshot,
) {
  const sourceLabel =
    node.sourceIds.length > 0 ? ` Source refs: ${node.sourceIds.join(", ")}.` : "";
  return `Node ${node.nodeId} is materialized in snapshot ${snapshot.snapshotId}.${sourceLabel}`;
}

function buildRelatedCounts(edges: MaterializedGraphRelationRecord[]) {
  const counts: Record<string, number> = {};
  for (const edge of edges) {
    counts[edge.sourceNodeId] = (counts[edge.sourceNodeId] ?? 0) + 1;
    counts[edge.targetNodeId] = (counts[edge.targetNodeId] ?? 0) + 1;
  }
  return counts;
}

function relatedNodeIdsForNode(edges: MaterializedGraphRelationRecord[], nodeId: string) {
  return edges
    .flatMap((edge) => {
      if (edge.sourceNodeId === nodeId) {
        return [edge.targetNodeId];
      }
      if (edge.targetNodeId === nodeId) {
        return [edge.sourceNodeId];
      }
      return [];
    })
    .filter((related, index, all) => all.indexOf(related) === index);
}

function excerpt(value: string) {
  const normalized = value.replace(/\s+/g, " ").trim();
  if (normalized.length <= 220) {
    return normalized;
  }
  return `${normalized.slice(0, 217)}...`;
}
