import type {
  WorkspaceAnswerResponse,
  WorkspaceEdgeDetail,
  WorkspaceEdgeSummary,
  WorkspaceEvidenceRef,
  WorkspaceNodeDetail,
  WorkspaceNodeSummary,
  WorkspaceProject,
} from "./types";

export interface CompletedResultLike {
  savedOutputPath: string | null;
  successCount: number;
  failedCount: number;
  markdown: string;
}

interface PageSection {
  pageLabel: string;
  content: string;
}

export function buildWorkspacePreview(
  result: CompletedResultLike | null,
  stale: boolean,
): WorkspaceProject | null {
  if (!result) {
    return null;
  }

  const title = inferProjectTitle(result.savedOutputPath, result.markdown);
  const pageSections = extractPageSections(result.markdown);
  const documentNode: WorkspaceNodeSummary = {
    id: "source:preview",
    label: title,
    kind: "source",
    confidence: 0.42,
    relatedCount: pageSections.length,
    evidenceCount: pageSections.length,
    position: { x: 48, y: 16 },
  };

  const pageNodes = pageSections.map((section, index) => {
    const perRow = 3;
    const row = Math.floor(index / perRow);
    const column = index % perRow;
    return {
      id: `page-${index + 1}`,
      label: section.pageLabel,
      kind: "page" as const,
      confidence: 0.56,
      relatedCount: 1,
      evidenceCount: 1,
      position: {
        x: 18 + column * 28,
        y: 44 + row * 24,
      },
    };
  });

  const nodes = [documentNode, ...pageNodes];
  const edges: WorkspaceEdgeSummary[] = pageNodes.map((node, index) => ({
    id: `edge-document-${node.id}`,
    sourceNodeId: "source:preview",
    targetNodeId: node.id,
    kind: "source_document",
    label: "Ingested from source",
    confidence: 0.42,
    evidenceCount: 1,
  }));
  const detailsByNodeId: Record<string, WorkspaceNodeDetail> = {};
  const edgeDetailsById: Record<string, WorkspaceEdgeDetail> = {};
  const answerByNodeId: Record<string, WorkspaceAnswerResponse> = {};

  const pageEvidence = pageSections.map((section, index) => ({
    id: `ev-page-${index + 1}`,
    pageLabel: section.pageLabel,
    snippet: excerpt(section.content),
    sourcePath: result.savedOutputPath,
  }));

  detailsByNodeId["source:preview"] = {
    node: documentNode,
    canonicalName: title,
    aliases: ["Latest import", "Preview project"],
    description:
      "This workspace is a graph-first preview built from the latest import. Etyma is showing the latest automatic ingest preview with visible evidence before making strong claims.",
    evidence: pageEvidence.slice(0, 3),
    actions: [],
    source: {
      workspaceId: "web-preview",
      sourceId: "preview",
      originalPath: result.savedOutputPath ?? title,
      sourcePath: result.savedOutputPath ?? title,
      markdownPath: result.savedOutputPath ?? title,
      format: "markdown",
      status: stale ? "stale" : "ingested",
      pageCount: pageSections.length,
      successCount: result.successCount,
      failedCount: result.failedCount,
      description: "",
      userContext: "",
      ingestInstruction: "",
      updatedAt: 0,
      manifestPath: null,
    },
  };
  answerByNodeId["source:preview"] = previewAnswer(
    "Etyma can already point you to the most relevant imported evidence, but grounded answer synthesis is still pending. Review the cited snippets before using this draft.",
    stale,
    pageEvidence.slice(0, 2),
    pageNodes.map((node) => node.id),
  );

  pageNodes.forEach((node, index) => {
    const evidence = [pageEvidence[index]].filter(Boolean);
    detailsByNodeId[node.id] = {
      node,
      canonicalName: node.label,
      aliases: [`Draft node ${index + 1}`],
      description:
        "Preview node derived from the latest import output. In the final knowledge workspace this inspector will show concept aliases and provenance-backed evidence.",
      evidence,
      actions: [],
    };
    answerByNodeId[node.id] = previewAnswer(
      `${node.label} is available as a draft workspace node. Etyma is keeping the answer conservative until concept linking and confidence scoring are connected.`,
      stale,
      evidence,
      [node.id],
    );
    edgeDetailsById[`edge-document-${node.id}`] = {
      edge: edges[index],
      explanation:
        "Preview edge only. The automatic ingest graph will replace this with real explainable relationships once relation extraction lands.",
      evidence,
    };
  });

  return {
    summary: {
      projectId: `preview:${title.toLowerCase().replace(/\s+/g, "-")}`,
      title,
      status: "preview",
      stale,
      summary: `${result.successCount} pages imported, ${result.failedCount} failed. Workspace preview is derived from the latest automatic ingest output and keeps evidence visible while full knowledge writing is still landing.`,
      documentCount: 1,
      nodeCount: nodes.length,
      relationshipCount: pageNodes.length,
      evidenceCount: pageEvidence.length,
    },
    nodes,
    edges,
    detailsByNodeId,
    edgeDetailsById,
    answerByNodeId,
  };
}

function previewAnswer(
  explanation: string,
  stale: boolean,
  citations: WorkspaceEvidenceRef[],
  relatedNodeIds: string[],
): WorkspaceAnswerResponse {
  return {
    status: stale ? "stale" : "low_confidence",
    text: null,
    explanation,
    citations,
    relatedNodeIds,
    suggestedActions: [
      {
        kind: "inspect_evidence" as const,
        label: "Inspect evidence",
        description:
          "Open the cited snippets in the inspector before using the draft answer.",
      },
      {
        kind: "ask_different_question" as const,
        label: "Wait for grounded answers",
        description:
          "Structured grounded answers will replace this preview once the knowledge writer lands.",
      },
    ],
  };
}

function extractPageSections(markdown: string): PageSection[] {
  const normalized = markdown.replace(/\r\n/g, "\n");
  const pageHeaderPattern = /^##\s+(Page\s+\d+)\s*$/gm;
  const matches = Array.from(normalized.matchAll(pageHeaderPattern));

  if (matches.length === 0) {
    return [
      {
        pageLabel: "Imported text",
        content: normalized,
      },
    ];
  }

  return matches.map((match, index) => {
    const start = match.index ?? 0;
    const contentStart = start + match[0].length;
    const nextStart = matches[index + 1]?.index ?? normalized.length;
    return {
      pageLabel: match[1],
      content: normalized.slice(contentStart, nextStart).trim(),
    };
  });
}

function inferProjectTitle(savedOutputPath: string | null, markdown: string): string {
  if (savedOutputPath) {
    const directoryName = savedOutputPath.split("/").filter(Boolean).pop();
    if (directoryName) {
      return directoryName.replace(/_[0-9-]+$/, "");
    }
  }

  const firstHeading = markdown.match(/^#\s+(.+)$/m)?.[1]?.trim();
  return firstHeading || "Latest import";
}

function excerpt(value: string, maxLength = 180): string {
  const compact = value.replace(/\s+/g, " ").trim();
  if (!compact) {
    return "No evidence snippet is available yet.";
  }
  if (compact.length <= maxLength) {
    return compact;
  }
  return `${compact.slice(0, maxLength - 1).trimEnd()}…`;
}
