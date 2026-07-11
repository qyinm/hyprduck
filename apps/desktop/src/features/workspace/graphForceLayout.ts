import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";

import type { SigmaWorkspaceGraph } from "./graphologyAdapter";
import type { WorkspaceNodeKind } from "./types";

export interface ForceNode extends SimulationNodeDatum {
  id: string;
  x: number;
  y: number;
  radius: number;
  clusterId: string;
  nodeKind: WorkspaceNodeKind;
  anchorX: number;
  anchorY: number;
}

export interface ForceLink extends SimulationLinkDatum<ForceNode> {
  source: string | ForceNode;
  target: string | ForceNode;
  edgeKind: string;
  sameCluster: boolean;
}

export function seedForceNodes(
  graph: SigmaWorkspaceGraph,
  storedPositions: Record<string, { x: number; y: number }>,
  currentPositions: Record<string, { x: number; y: number }>,
): {
  nodes: ForceNode[];
  links: ForceLink[];
} {
  const nodes = graph.nodes().map((nodeId) => {
    const data = graph.getNodeAttributes(nodeId);
    const stored = storedPositions[nodeId];
    const current = currentPositions[nodeId];
    const start = stored ?? current ?? { x: data.x, y: data.y };
    return {
      id: nodeId,
      x: finiteGraphPosition(start.x),
      y: finiteGraphPosition(start.y),
      radius: data.nodeKind === "source" || data.nodeKind === "document" ? 0.105 : 0.08,
      clusterId: data.clusterId,
      nodeKind: data.nodeKind,
      anchorX: data.x,
      anchorY: data.y,
      fx: stored ? finiteGraphPosition(stored.x) : undefined,
      fy: stored ? finiteGraphPosition(stored.y) : undefined,
    };
  });
  const links = graph.edges().map((edgeId) => {
    const source = graph.source(edgeId);
    const target = graph.target(edgeId);
    return {
      source,
      target,
      edgeKind: graph.getEdgeAttribute(edgeId, "edgeKind"),
      sameCluster:
        graph.getNodeAttribute(source, "clusterId") ===
        graph.getNodeAttribute(target, "clusterId"),
    };
  });
  return { nodes, links };
}

export function buildForceSimulation(
  nodes: ForceNode[],
  links: ForceLink[],
): Simulation<ForceNode, ForceLink> {
  return forceSimulation<ForceNode>(nodes)
    .alpha(0.85)
    .alphaMin(0.008)
    .alphaDecay(0.045)
    .velocityDecay(0.5)
    .force(
      "charge",
      forceManyBody<ForceNode>()
        .strength((node) =>
          node.nodeKind === "source" || node.nodeKind === "document"
            ? -0.34
            : -0.22,
        )
        .distanceMin(0.08)
        .distanceMax(2.4),
    )
    .force(
      "link",
      forceLink<ForceNode, ForceLink>(links)
        .id((node) => node.id)
        .distance((link) => {
          if (link.edgeKind === "source_document") {
            return 0.28;
          }
          return link.sameCluster ? 0.58 : 1.55;
        })
        .strength((link) => {
          if (link.edgeKind === "source_document") {
            return 0.75;
          }
          return link.sameCluster ? 0.045 : 0.006;
        }),
    )
    .force(
      "collide",
      forceCollide<ForceNode>()
        .radius((node) => node.radius + 0.055)
        .strength(0.85)
        .iterations(2),
    )
    .force("clusterX", forceX<ForceNode>((node) => node.anchorX).strength(0.075))
    .force("clusterY", forceY<ForceNode>((node) => node.anchorY).strength(0.075))
    .force("center", forceCenter<ForceNode>(0, 0).strength(0.012));
}

export function forceNodesToPositions(
  nodes: ForceNode[],
): Record<string, { x: number; y: number }> {
  return Object.fromEntries(
    nodes.map((node) => [
      node.id,
      {
        x: finiteGraphPosition(node.x),
        y: finiteGraphPosition(node.y),
      },
    ]),
  );
}

export function graphLayoutKey(graph: SigmaWorkspaceGraph): string {
  return JSON.stringify({
    nodes: graph.nodes().sort(),
    edges: graph
      .edges()
      .map((edge) => [
        graph.source(edge),
        graph.target(edge),
        graph.getEdgeAttribute(edge, "edgeKind"),
      ])
      .sort((a, b) => a.join(":").localeCompare(b.join(":"))),
  });
}

export function graphPositionBounds(
  positions: Record<string, { x: number; y: number }>,
): { minX: number; minY: number; maxX: number; maxY: number } | null {
  const values = Object.values(positions).filter(
    (position) => Number.isFinite(position.x) && Number.isFinite(position.y),
  );
  if (values.length === 0) {
    return null;
  }

  return {
    minX: Math.min(...values.map((position) => position.x)),
    minY: Math.min(...values.map((position) => position.y)),
    maxX: Math.max(...values.map((position) => position.x)),
    maxY: Math.max(...values.map((position) => position.y)),
  };
}

export function finiteGraphPosition(value: number, fallback = 0): number {
  return Number.isFinite(value) ? value : fallback;
}
