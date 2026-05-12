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

export interface BuildSigmaGraphScope {
  mode: "global" | "local";
  centerNodeId: string | null;
  depth?: number;
}

const NODE_COLORS: Record<WorkspaceNodeKind, string> = {
  source: "#111111",
  document: "#111111",
  page: "#6b7280",
  concept: "#111111",
};

const NODE_BORDER_COLORS: Record<WorkspaceNodeKind, string> = {
  source: "#111111",
  document: "#111111",
  page: "#9ca3af",
  concept: "#111111",
};

export type SigmaWorkspaceGraph = Graph<SigmaNodeAttributes, SigmaEdgeAttributes>;

export function buildSigmaGraph(
  project: WorkspaceProject,
  selection: BuildSigmaGraphSelection,
  scope: BuildSigmaGraphScope = { mode: "global", centerNodeId: null },
): SigmaWorkspaceGraph {
  const graph: SigmaWorkspaceGraph = new Graph({
    allowSelfLoops: false,
    multi: true,
    type: "mixed",
  });
  const visibleProject = scopedProject(project, scope);
  const layoutPositions = buildComponentLayout(visibleProject);

  for (const [index, node] of visibleProject.nodes.entries()) {
    const selected = selection.selectedNodeId === node.id;
    const layoutPosition =
      layoutPositions[node.id] ??
      fallbackNodePosition(index, visibleProject.nodes.length);
    graph.addNode(node.id, {
      x: layoutPosition.x,
      y: layoutPosition.y,
      label: node.label,
      size: nodeSize(node.kind, node.evidenceCount, selected),
      color: selected ? "#111111" : NODE_COLORS[node.kind],
      borderColor: selected ? "#111111" : NODE_BORDER_COLORS[node.kind],
      forceLabel: selected || node.kind === "source" || node.kind === "document",
      highlighted: selected,
      hidden: false,
      selected,
      nodeKind: node.kind,
      evidenceCount: node.evidenceCount,
      confidence: node.confidence,
      relatedCount: node.relatedCount,
      zIndex: selected
        ? 10
        : node.kind === "source" || node.kind === "document"
          ? 5
          : 1,
    });
  }

  for (const edge of visibleProject.edges) {
    if (!graph.hasNode(edge.sourceNodeId) || !graph.hasNode(edge.targetNodeId)) {
      continue;
    }

    if (edge.sourceNodeId === edge.targetNodeId) {
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

function scopedProject(
  project: WorkspaceProject,
  scope: BuildSigmaGraphScope,
): WorkspaceProject {
  if (scope.mode === "global" || !scope.centerNodeId) {
    return project;
  }

  const nodeSet = new Set(project.nodes.map((node) => node.id));
  if (!nodeSet.has(scope.centerNodeId)) {
    return project;
  }

  const visibleNodeIds = localNodeIds(
    project,
    scope.centerNodeId,
    scope.depth ?? 1,
  );
  return {
    ...project,
    nodes: project.nodes.filter((node) => visibleNodeIds.has(node.id)),
    edges: project.edges.filter(
      (edge) =>
        visibleNodeIds.has(edge.sourceNodeId) &&
        visibleNodeIds.has(edge.targetNodeId),
    ),
  };
}

function localNodeIds(
  project: WorkspaceProject,
  centerNodeId: string,
  depth: number,
): Set<string> {
  const neighbors = new Map<string, Set<string>>(
    project.nodes.map((node) => [node.id, new Set<string>()]),
  );
  for (const edge of project.edges) {
    neighbors.get(edge.sourceNodeId)?.add(edge.targetNodeId);
    neighbors.get(edge.targetNodeId)?.add(edge.sourceNodeId);
  }

  const visible = new Set<string>([centerNodeId]);
  let frontier = new Set<string>([centerNodeId]);
  for (let level = 0; level < Math.max(0, depth); level += 1) {
    const nextFrontier = new Set<string>();
    for (const nodeId of frontier) {
      for (const neighborId of neighbors.get(nodeId) ?? []) {
        if (!visible.has(neighborId)) {
          visible.add(neighborId);
          nextFrontier.add(neighborId);
        }
      }
    }
    frontier = nextFrontier;
  }

  return visible;
}

function nodeSize(
  kind: WorkspaceNodeKind,
  evidenceCount: number,
  selected: boolean,
): number {
  const base =
    kind === "source" || kind === "document" ? 11 : kind === "concept" ? 9 : 7;
  const evidenceBoost = Math.min(4, Math.max(0, evidenceCount - 1));
  return base + evidenceBoost + (selected ? 2 : 0);
}

function fallbackNodePosition(index: number, total: number): { x: number; y: number } {
  if (index === 0) {
    return { x: 0, y: 0 };
  }

  const radius = 0.58;
  const angle =
    ((index - 1) / Math.max(1, total - 1)) * Math.PI * 2 - Math.PI / 2;
  return {
    x: Math.cos(angle) * radius,
    y: Math.sin(angle) * radius,
  };
}

function buildComponentLayout(
  project: WorkspaceProject,
): Record<string, { x: number; y: number }> {
  const nodeIds = project.nodes.map((node) => node.id);
  const nodeSet = new Set(nodeIds);
  const neighbors = new Map<string, Set<string>>(
    nodeIds.map((nodeId) => [nodeId, new Set<string>()]),
  );

  for (const edge of project.edges) {
    if (
      edge.sourceNodeId === edge.targetNodeId ||
      !nodeSet.has(edge.sourceNodeId) ||
      !nodeSet.has(edge.targetNodeId)
    ) {
      continue;
    }

    neighbors.get(edge.sourceNodeId)?.add(edge.targetNodeId);
    neighbors.get(edge.targetNodeId)?.add(edge.sourceNodeId);
  }

  const visited = new Set<string>();
  const components: string[][] = [];
  for (const nodeId of nodeIds) {
    if (visited.has(nodeId)) {
      continue;
    }

    const component: string[] = [];
    const stack = [nodeId];
    visited.add(nodeId);
    while (stack.length > 0) {
      const current = stack.pop();
      if (!current) {
        continue;
      }
      component.push(current);
      for (const next of neighbors.get(current) ?? []) {
        if (!visited.has(next)) {
          visited.add(next);
          stack.push(next);
        }
      }
    }
    components.push(component.sort(compareNodeIds));
  }

  components.sort((a, b) => b.length - a.length || compareNodeIds(a[0], b[0]));

  const positions: Record<string, { x: number; y: number }> = {};
  components.forEach((component, componentIndex) => {
    const center = componentCenter(componentIndex, components.length);
    const radius = componentRadius(component.length);

    component.forEach((nodeId, nodeIndex) => {
      if (component.length === 1) {
        positions[nodeId] = center;
        return;
      }

      const angle =
        (nodeIndex / component.length) * Math.PI * 2 -
        Math.PI / 2 +
        stableAngleOffset(component[0]);
      const distance = component.length === 2 ? radius * 0.72 : radius;
      positions[nodeId] = {
        x: clampLayoutPosition(center.x + Math.cos(angle) * distance),
        y: clampLayoutPosition(center.y + Math.sin(angle) * distance),
      };
    });
  });

  return relaxOverlappingPositions(positions, nodeIds);
}

function componentCenter(index: number, total: number): { x: number; y: number } {
  if (total === 1) {
    return { x: 0, y: 0 };
  }

  const ring = Math.floor(index / 8);
  const ringIndex = index % 8;
  const angle = (ringIndex / Math.min(8, total)) * Math.PI * 2 - Math.PI / 2;
  const radius = Math.min(1.08, 0.54 + ring * 0.28);
  return {
    x: Math.cos(angle) * radius,
    y: Math.sin(angle) * radius,
  };
}

function componentRadius(size: number): number {
  if (size <= 1) {
    return 0;
  }
  return Math.min(0.38, Math.max(0.18, Math.sqrt(size) * 0.11));
}

function stableAngleOffset(seed: string): number {
  let hash = 0;
  for (let index = 0; index < seed.length; index += 1) {
    hash = (hash * 31 + seed.charCodeAt(index)) >>> 0;
  }
  return ((hash % 360) / 360) * Math.PI * 0.22;
}

function relaxOverlappingPositions(
  positions: Record<string, { x: number; y: number }>,
  nodeIds: string[],
): Record<string, { x: number; y: number }> {
  const relaxed: Record<string, { x: number; y: number }> = Object.fromEntries(
    nodeIds.map((nodeId) => [nodeId, { ...positions[nodeId] }]),
  );
  const minDistance = 0.16;

  for (let iteration = 0; iteration < 18; iteration += 1) {
    for (let sourceIndex = 0; sourceIndex < nodeIds.length; sourceIndex += 1) {
      for (
        let targetIndex = sourceIndex + 1;
        targetIndex < nodeIds.length;
        targetIndex += 1
      ) {
        const sourceId = nodeIds[sourceIndex];
        const targetId = nodeIds[targetIndex];
        const source = relaxed[sourceId];
        const target = relaxed[targetId];
        const deltaX = target.x - source.x;
        const deltaY = target.y - source.y;
        const distance = Math.hypot(deltaX, deltaY);

        if (distance >= minDistance) {
          continue;
        }

        const fallbackAngle = stableAngleOffset(`${sourceId}:${targetId}`) * 8;
        const directionX =
          distance === 0 ? Math.cos(fallbackAngle) : deltaX / distance;
        const directionY =
          distance === 0 ? Math.sin(fallbackAngle) : deltaY / distance;
        const push = (minDistance - distance) / 2;

        source.x = clampLayoutPosition(source.x - directionX * push);
        source.y = clampLayoutPosition(source.y - directionY * push);
        target.x = clampLayoutPosition(target.x + directionX * push);
        target.y = clampLayoutPosition(target.y + directionY * push);
      }
    }
  }

  return relaxed;
}

function compareNodeIds(a: string, b: string): number {
  return a.localeCompare(b);
}

function clampLayoutPosition(value: number): number {
  return Math.max(-1.35, Math.min(1.35, value));
}
