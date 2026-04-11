import type {
  WorkspaceAnswerResponse,
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
    id: "document",
    label: title,
    kind: "document",
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
  const detailsByNodeId: Record<string, WorkspaceNodeDetail> = {};
  const answerByNodeId: Record<string, WorkspaceAnswerResponse> = {};

  const pageEvidence = pageSections.map((section, index) => ({
    id: `ev-page-${index + 1}`,
    pageLabel: section.pageLabel,
    snippet: excerpt(section.content),
    sourcePath: result.savedOutputPath,
  }));

  detailsByNodeId.document = {
    node: documentNode,
    canonicalName: title,
    aliases: ["Latest import", "Preview project"],
    description:
      "This workspace is a graph-first preview built from the latest import. The compile-backed knowledge layer is not wired yet, so DuckDocs shows visible evidence before making strong claims.",
    evidence: pageEvidence.slice(0, 3),
    correctionActions: disabledCorrectionActions(
      "Correction actions unlock once project compile and merge policy are connected.",
    ),
  };
  answerByNodeId.document = previewAnswer(
    "DuckDocs can already point you to the most relevant imported evidence, but compile-backed grounded answers are still pending. Review the cited snippets before trusting this draft.",
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
        "Preview node derived from the latest import output. In the final knowledge workspace this inspector will show concept aliases, merge control, and provenance-backed evidence.",
      evidence,
      correctionActions: disabledCorrectionActions(
        "Corrections are visible here so the tri-pane workflow can land before backend apply flows are wired.",
      ),
    };
    answerByNodeId[node.id] = previewAnswer(
      `${node.label} is available as a draft workspace node. DuckDocs is keeping the answer conservative until concept compile and confidence scoring are connected.`,
      stale,
      evidence,
      [node.id],
    );
  });

  return {
    summary: {
      projectId: `preview:${title.toLowerCase().replace(/\s+/g, "-")}`,
      title,
      status: "preview",
      stale,
      summary: `${result.successCount} pages imported, ${result.failedCount} failed. Workspace preview is derived from the latest markdown package and keeps evidence visible while the knowledge compiler is still landing.`,
      documentCount: 1,
      nodeCount: nodes.length,
      relationshipCount: pageNodes.length,
      evidenceCount: pageEvidence.length,
    },
    nodes,
    detailsByNodeId,
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
          "Open the cited snippets in the inspector before trusting the draft answer.",
      },
      {
        kind: "ask_different_question" as const,
        label: "Wait for compile-backed answers",
        description:
          "Structured grounded answers will replace this preview once the project compiler lands.",
      },
    ],
  };
}

function disabledCorrectionActions(reason: string) {
  return [
    {
      kind: "merge" as const,
      label: "Merge",
      disabledReason: reason,
    },
    {
      kind: "keep_separate" as const,
      label: "Keep Separate",
      disabledReason: reason,
    },
    {
      kind: "rename" as const,
      label: "Rename",
      disabledReason: reason,
    },
  ];
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
