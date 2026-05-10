import Graph from "graphology";

import type { WorkspaceProject, WorkspaceNodeKind } from "./types";

export interface SigmaNodeAttributes {
  x: number;
  y: number;
  label: string;
  size: number;
  color: string;
  borderColor: string;
  forceLabel: boolean;
  highlighted: boolean;
  hidden: boolean;
  selected: boolean;
  nodeKind: WorkspaceNodeKind;
  evidenceCount: number;
  confidence: number | null;
  relatedCount: number;
  zIndex: number;
}

export interface SigmaEdgeAttributes {
  label: string;
  size: number;
  color: string;
  hidden: boolean;
  selected: boolean;
  edgeKind: string;
  evidenceCount: number;
  confidence: number | null;
  type?: string;
}

interface BuildSigmaGraphSelection {
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
}

const NODE_COLORS: Record<WorkspaceNodeKind, string> = {
  document: "#111111",
  page: "#6b7280",
  concept: "#111111",
};

const NODE_BORDER_COLORS: Record<WorkspaceNodeKind, string> = {
  document: "#111111",
  page: "#9ca3af",
  concept: "#111111",
};

export type SigmaWorkspaceGraph = Graph<SigmaNodeAttributes, SigmaEdgeAttributes>;

export function buildSigmaGraph(
  project: WorkspaceProject,
  selection: BuildSigmaGraphSelection,
): SigmaWorkspaceGraph {
  const graph: SigmaWorkspaceGraph = new Graph({
    allowSelfLoops: false,
    multi: true,
    type: "mixed",
  });

  for (const [index, node] of project.nodes.entries()) {
    const selected = selection.selectedNodeId === node.id;
    const fallbackPosition = fallbackNodePosition(index, project.nodes.length);
    graph.addNode(node.id, {
      x: normalizeX(node.position?.x, fallbackPosition.x),
      y: normalizeY(node.position?.y, fallbackPosition.y),
      label: node.label,
      size: nodeSize(node.kind, node.evidenceCount, selected),
      color: selected ? "#111111" : NODE_COLORS[node.kind],
      borderColor: selected ? "#111111" : NODE_BORDER_COLORS[node.kind],
      forceLabel: selected || node.kind === "document",
      highlighted: selected,
      hidden: false,
      selected,
      nodeKind: node.kind,
      evidenceCount: node.evidenceCount,
      confidence: node.confidence,
      relatedCount: node.relatedCount,
      zIndex: selected ? 10 : node.kind === "document" ? 5 : 1,
    });
  }

  for (const edge of project.edges) {
    if (!graph.hasNode(edge.sourceNodeId) || !graph.hasNode(edge.targetNodeId)) {
      continue;
    }

    const selected = selection.selectedEdgeId === edge.id;
    graph.addEdgeWithKey(edge.id, edge.sourceNodeId, edge.targetNodeId, {
      label: edge.label,
      size: selected ? 3 : edge.kind === "source_document" ? 1.8 : 1.4,
      color: selected ? "#111111" : edge.kind === "source_document" ? "#cbd5e1" : "#9ca3af",
      hidden: false,
      selected,
      edgeKind: edge.kind,
      evidenceCount: edge.evidenceCount,
      confidence: edge.confidence,
    });
  }

  return graph;
}

function normalizeX(percent: number | undefined, fallbackPercent: number): number {
  const value = typeof percent === "number" && Number.isFinite(percent) ? percent : fallbackPercent;
  return (value - 50) / 50;
}

function normalizeY(percent: number | undefined, fallbackPercent: number): number {
  const value = typeof percent === "number" && Number.isFinite(percent) ? percent : fallbackPercent;
  return (50 - value) / 50;
}

function nodeSize(
  kind: WorkspaceNodeKind,
  evidenceCount: number,
  selected: boolean,
): number {
  const base = kind === "document" ? 11 : kind === "concept" ? 9 : 7;
  const evidenceBoost = Math.min(4, Math.max(0, evidenceCount - 1));
  return base + evidenceBoost + (selected ? 2 : 0);
}

function fallbackNodePosition(index: number, total: number): { x: number; y: number } {
  if (index === 0) {
    return { x: 50, y: 22 };
  }

  const radius = 30;
  const angle = ((index - 1) / Math.max(1, total - 1)) * Math.PI * 2 - Math.PI / 2;
  return {
    x: 50 + Math.cos(angle) * radius,
    y: 54 + Math.sin(angle) * radius,
  };
}
