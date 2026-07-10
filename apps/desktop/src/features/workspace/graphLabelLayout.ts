import type { SigmaNodeAttributes, SigmaWorkspaceGraph } from "./graphologyAdapter";
import type { GraphViewport } from "./graphViewport";

export function computeVisibleLabels(params: {
  graph: SigmaWorkspaceGraph;
  nodes: string[];
  positions: Record<string, { x: number; y: number }>;
  viewport: GraphViewport;
  selectedNodeId: string | null;
  hoveredNodeId: string | null;
}): Set<string> {
  const { graph, nodes, positions, viewport, selectedNodeId, hoveredNodeId } =
    params;
  const occupied: Array<{ x1: number; y1: number; x2: number; y2: number }> = [];
  const visible = new Set<string>();
  const candidates = nodes
    .map((nodeId) => {
      const data = graph.getNodeAttributes(nodeId);
      const selected = selectedNodeId === nodeId;
      const hovered = hoveredNodeId === nodeId;
      if (!labelEligible(data, viewport.zoom, selected, hovered)) {
        return null;
      }
      return {
        nodeId,
        data,
        position: positions[nodeId] ?? { x: data.x, y: data.y },
        selected,
        hovered,
        priority:
          data.labelPriority + (selected ? 10_000 : 0) + (hovered ? 9_000 : 0),
      };
    })
    .filter((candidate): candidate is NonNullable<typeof candidate> => Boolean(candidate))
    .sort((a, b) => b.priority - a.priority || a.nodeId.localeCompare(b.nodeId));

  for (const candidate of candidates) {
    const rect = estimatedLabelRect({
      label: candidate.data.shortLabel,
      position: candidate.position,
      viewport,
    });
    const forced = candidate.selected || candidate.hovered;
    if (!forced && occupied.some((other) => rectsOverlap(rect, other))) {
      continue;
    }
    visible.add(candidate.nodeId);
    occupied.push(rect);
  }

  return visible;
}

function labelEligible(
  data: SigmaNodeAttributes,
  zoom: number,
  selected: boolean,
  hovered: boolean,
): boolean {
  if (selected || hovered) {
    return true;
  }
  if (data.nodeKind === "source" || data.nodeKind === "document") {
    return zoom >= 0.18;
  }
  if (zoom < 0.75) {
    return false;
  }
  if (zoom < 1.15) {
    return data.relatedCount >= 4 || data.evidenceCount >= 3;
  }
  return true;
}

function estimatedLabelRect(params: {
  label: string;
  position: { x: number; y: number };
  viewport: GraphViewport;
}): { x1: number; y1: number; x2: number; y2: number } {
  const { label, position, viewport } = params;
  const x = toPercentX(position.x) * viewport.zoom + viewport.panX;
  const y = toPercentY(position.y) * viewport.zoom + viewport.panY;
  const fontSize = 2.15;
  const width = Math.max(4, displayCells(label) * fontSize * 0.42);
  const height = fontSize * 1.25;
  const gap = 2.8;

  return {
    x1: x - width / 2,
    x2: x + width / 2,
    y1: y + gap,
    y2: y + gap + height,
  };
}

function rectsOverlap(
  a: { x1: number; y1: number; x2: number; y2: number },
  b: { x1: number; y1: number; x2: number; y2: number },
): boolean {
  return a.x1 < b.x2 && a.x2 > b.x1 && a.y1 < b.y2 && a.y2 > b.y1;
}

function displayCells(value: string): number {
  let cells = 0;
  for (const char of Array.from(value)) {
    cells += /[\u1100-\u11FF\u2E80-\uA4CF\uAC00-\uD7A3\uF900-\uFAFF\uFE10-\uFE6F\uFF00-\uFF60\uFFE0-\uFFE6]/.test(
      char,
    )
      ? 2
      : 1;
  }
  return cells;
}

export function toPercentX(value: number): number {
  return value * 50 + 50;
}

export function toPercentY(value: number): number {
  return 50 - value * 50;
}
