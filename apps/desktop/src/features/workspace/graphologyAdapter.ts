import Graph from "graphology";

import type { WorkspaceProject, WorkspaceNodeKind } from "./types";

export interface SigmaNodeAttributes {
  x: number;
  y: number;
  label: string;
  shortLabel: string;
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
  clusterId: string;
  clusterAnchorId: string;
  labelPriority: number;
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

export interface BuildSigmaGraphSelection {
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
  const layout = buildClusterLayout(visibleProject);

  for (const [index, node] of visibleProject.nodes.entries()) {
    const selected = selection.selectedNodeId === node.id;
    const layoutPosition =
      layout.positions[node.id] ??
      fallbackNodePosition(index, visibleProject.nodes.length);
    const clusterId = layout.clusterByNodeId[node.id] ?? `node:${node.id}`;
    graph.addNode(node.id, {
      x: layoutPosition.x,
      y: layoutPosition.y,
      label: node.label,
      shortLabel: formatGraphLabel(node.label, node.kind),
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
      clusterId,
      clusterAnchorId: layout.anchorByClusterId[clusterId] ?? node.id,
      labelPriority: graphLabelPriority(
        node.kind,
        node.evidenceCount,
        node.relatedCount,
        selected,
      ),
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

interface ClusterLayoutResult {
  positions: Record<string, { x: number; y: number }>;
  clusterByNodeId: Record<string, string>;
  anchorByClusterId: Record<string, string>;
}

function buildClusterLayout(project: WorkspaceProject): ClusterLayoutResult {
  const nodeIds = project.nodes.map((node) => node.id);
  const nodeSet = new Set(nodeIds);
  const sourceAnchors = sourceClusterAnchors(project).filter((nodeId) =>
    nodeSet.has(nodeId),
  );

  if (sourceAnchors.length > 0) {
    return buildSourceAnchoredClusterLayout(project, nodeIds, nodeSet, sourceAnchors);
  }

  return buildConnectedComponentLayout(project, nodeIds, nodeSet);
}

function buildSourceAnchoredClusterLayout(
  project: WorkspaceProject,
  nodeIds: string[],
  nodeSet: Set<string>,
  sourceAnchors: string[],
): ClusterLayoutResult {
  const sourceNeighbors = new Map<string, Set<string>>(
    nodeIds.map((nodeId) => [nodeId, new Set<string>()]),
  );
  const relatedNeighbors = new Map<string, Set<string>>(
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

    const targetMap =
      edge.kind === "source_document" ? sourceNeighbors : relatedNeighbors;
    targetMap.get(edge.sourceNodeId)?.add(edge.targetNodeId);
    targetMap.get(edge.targetNodeId)?.add(edge.sourceNodeId);
  }

  const assigned = new Set<string>();
  const anchorSet = new Set(sourceAnchors);
  const clusters = sourceAnchors.map((anchorId) => {
    const members = sourceDocumentReachableNodes(
      anchorId,
      sourceNeighbors,
      anchorSet,
    );
    members.add(anchorId);
    for (const member of [...members]) {
      if (assigned.has(member)) {
        members.delete(member);
      }
    }
    members.add(anchorId);
    for (const member of members) {
      assigned.add(member);
    }
    return {
      anchorId,
      members: [...members].sort(compareNodeIds),
    };
  });

  let changed = true;
  while (changed) {
    changed = false;
    for (const nodeId of nodeIds) {
      if (assigned.has(nodeId)) {
        continue;
      }

      const anchorId = nearestAssignedAnchor(nodeId, relatedNeighbors, clusters);
      if (!anchorId) {
        continue;
      }

      const cluster = clusters.find((entry) => entry.anchorId === anchorId);
      if (!cluster) {
        continue;
      }

      cluster.members.push(nodeId);
      cluster.members.sort(compareNodeIds);
      assigned.add(nodeId);
      changed = true;
    }
  }

  const unassigned = nodeIds.filter((nodeId) => !assigned.has(nodeId));
  const fallback = buildUnassignedClusters(unassigned, relatedNeighbors);
  const allClusters = [
    ...clusters.filter((cluster) => cluster.members.length > 0),
    ...fallback,
  ].sort((a, b) => compareNodeIds(a.anchorId, b.anchorId));

  return layoutClusters(allClusters, nodeIds);
}

function sourceClusterAnchors(project: WorkspaceProject): string[] {
  const sourceDocumentTargets = new Set<string>();
  for (const edge of project.edges) {
    if (edge.kind === "source_document") {
      sourceDocumentTargets.add(edge.targetNodeId);
    }
  }

  return project.nodes
    .filter((node) => {
      if (node.kind === "source") {
        return true;
      }
      if (node.kind === "document") {
        return !sourceDocumentTargets.has(node.id);
      }
      return false;
    })
    .map((node) => node.id)
    .sort(compareNodeIds);
}

function sourceDocumentReachableNodes(
  anchorId: string,
  neighbors: Map<string, Set<string>>,
  anchorSet: Set<string>,
): Set<string> {
  const visited = new Set<string>([anchorId]);
  const stack = [anchorId];
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) {
      continue;
    }
    for (const next of neighbors.get(current) ?? []) {
      if (next !== anchorId && anchorSet.has(next)) {
        continue;
      }
      if (!visited.has(next)) {
        visited.add(next);
        stack.push(next);
      }
    }
  }
  return visited;
}

function nearestAssignedAnchor(
  nodeId: string,
  neighbors: Map<string, Set<string>>,
  clusters: Array<{ anchorId: string; members: string[] }>,
): string | null {
  const memberToAnchor = new Map<string, string>();
  for (const cluster of clusters) {
    for (const member of cluster.members) {
      memberToAnchor.set(member, cluster.anchorId);
    }
  }

  const visited = new Set<string>([nodeId]);
  let frontier = new Set<string>([nodeId]);
  for (let depth = 0; depth < 3; depth += 1) {
    const nextFrontier = new Set<string>();
    const anchors = new Set<string>();
    for (const current of frontier) {
      for (const next of neighbors.get(current) ?? []) {
        const anchorId = memberToAnchor.get(next);
        if (anchorId) {
          anchors.add(anchorId);
        }
        if (!visited.has(next)) {
          visited.add(next);
          nextFrontier.add(next);
        }
      }
    }
    if (anchors.size > 0) {
      return [...anchors].sort(compareNodeIds)[0] ?? null;
    }
    frontier = nextFrontier;
  }
  return null;
}

function buildUnassignedClusters(
  nodeIds: string[],
  neighbors: Map<string, Set<string>>,
): Array<{ anchorId: string; members: string[] }> {
  const nodeIdSet = new Set(nodeIds);
  const visited = new Set<string>();
  const clusters: Array<{ anchorId: string; members: string[] }> = [];
  for (const nodeId of [...nodeIds].sort(compareNodeIds)) {
    if (visited.has(nodeId)) {
      continue;
    }

    const members: string[] = [];
    const stack = [nodeId];
    visited.add(nodeId);
    while (stack.length > 0) {
      const current = stack.pop();
      if (!current) {
        continue;
      }
      members.push(current);
      for (const next of neighbors.get(current) ?? []) {
        if (nodeIdSet.has(next) && !visited.has(next)) {
          visited.add(next);
          stack.push(next);
        }
      }
    }
    members.sort(compareNodeIds);
    clusters.push({ anchorId: members[0] ?? nodeId, members });
  }
  return clusters;
}

function buildConnectedComponentLayout(
  project: WorkspaceProject,
  nodeIds: string[],
  nodeSet: Set<string>,
): ClusterLayoutResult {
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

  components.sort((a, b) => compareNodeIds(a[0], b[0]));

  return layoutClusters(
    components.map((members) => ({ anchorId: members[0] ?? "", members })),
    nodeIds,
  );
}

function layoutClusters(
  clusters: Array<{ anchorId: string; members: string[] }>,
  nodeIds: string[],
): ClusterLayoutResult {
  const positions: Record<string, { x: number; y: number }> = {};
  const clusterByNodeId: Record<string, string> = {};
  const anchorByClusterId: Record<string, string> = {};
  clusters.forEach((cluster, componentIndex) => {
    const component = cluster.members;
    const center = componentCenter(componentIndex, clusters.length);
    const radius = componentRadius(component.length);
    const clusterId = `cluster:${cluster.anchorId}`;
    anchorByClusterId[clusterId] = cluster.anchorId;

    component.forEach((nodeId, nodeIndex) => {
      clusterByNodeId[nodeId] = clusterId;
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
        x: center.x + Math.cos(angle) * distance,
        y: center.y + Math.sin(angle) * distance,
      };
    });
  });

  return {
    positions: relaxOverlappingPositions(positions, nodeIds),
    clusterByNodeId,
    anchorByClusterId,
  };
}

function componentCenter(index: number, total: number): { x: number; y: number } {
  if (total === 1) {
    return { x: 0, y: 0 };
  }

  const ring = Math.floor(index / 8);
  const ringIndex = index % 8;
  const angle = (ringIndex / Math.min(8, total)) * Math.PI * 2 - Math.PI / 2;
  const radius = 0.72 + ring * 0.52;
  return {
    x: Math.cos(angle) * radius,
    y: Math.sin(angle) * radius,
  };
}

function componentRadius(size: number): number {
  if (size <= 1) {
    return 0;
  }
  return Math.max(0.22, Math.sqrt(size) * 0.14);
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

        source.x -= directionX * push;
        source.y -= directionY * push;
        target.x += directionX * push;
        target.y += directionY * push;
      }
    }
  }

  return relaxed;
}

function compareNodeIds(a: string, b: string): number {
  return a.localeCompare(b);
}

function graphLabelPriority(
  kind: WorkspaceNodeKind,
  evidenceCount: number,
  relatedCount: number,
  selected: boolean,
): number {
  if (selected) {
    return 10_000;
  }
  if (kind === "source" || kind === "document") {
    return 5_000;
  }
  return relatedCount * 30 + evidenceCount * 20;
}

function formatGraphLabel(rawLabel: string, kind: WorkspaceNodeKind): string {
  const normalized = rawLabel.trim().replace(/\s+/g, " ");
  const maxCells = kind === "source" || kind === "document" ? 24 : 18;
  if (displayCells(normalized) <= maxCells) {
    return normalized;
  }

  const extensionMatch =
    kind === "source" || kind === "document"
      ? normalized.match(/(\.[A-Za-z0-9]{1,8})$/)
      : null;
  const extension = extensionMatch?.[1] ?? "";
  const withoutExtension = extension
    ? normalized.slice(0, -extension.length)
    : normalized;
  const extensionCells = displayCells(extension);
  const suffixBudget = extension ? Math.min(8, Math.floor(maxCells * 0.35)) : 6;
  const prefixBudget = Math.max(4, maxCells - suffixBudget - extensionCells - 1);

  return `${takeDisplayCells(withoutExtension, prefixBudget, "start")}…${takeDisplayCells(
    withoutExtension,
    suffixBudget,
    "end",
  )}${extension}`;
}

function displayCells(value: string): number {
  let cells = 0;
  for (const char of Array.from(value)) {
    cells += isWideChar(char) ? 2 : 1;
  }
  return cells;
}

function takeDisplayCells(
  value: string,
  maxCells: number,
  direction: "start" | "end",
): string {
  const chars = Array.from(value);
  const ordered = direction === "start" ? chars : chars.reverse();
  let used = 0;
  const result: string[] = [];

  for (const char of ordered) {
    const width = isWideChar(char) ? 2 : 1;
    if (used + width > maxCells) {
      break;
    }
    result.push(char);
    used += width;
  }

  return direction === "start" ? result.join("") : result.reverse().join("");
}

function isWideChar(char: string): boolean {
  return /[\u1100-\u11FF\u2E80-\uA4CF\uAC00-\uD7A3\uF900-\uFAFF\uFE10-\uFE6F\uFF00-\uFF60\uFFE0-\uFFE6]/.test(
    char,
  );
}
