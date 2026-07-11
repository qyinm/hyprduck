import type { BuildSigmaGraphScope } from "./graphologyAdapter";
import { finiteGraphPosition } from "./graphForceLayout";

export interface StoredGraphLayout {
  version: 2;
  nodes: Record<string, { x: number; y: number; pinned: true; updatedAt: number }>;
}

/** Stable localStorage key format — do not change without a migration. */
export function graphPositionStorageKey(
  projectId: string,
  scope: BuildSigmaGraphScope,
): string {
  if (scope.mode === "local" && scope.centerNodeId) {
    return `etyma:graph-layout:v2:${projectId}:local:${scope.centerNodeId}`;
  }

  return `etyma:graph-layout:v2:${projectId}:global`;
}

export function readStoredGraphLayout(key: string): StoredGraphLayout {
  if (typeof window === "undefined") {
    return { version: 2, nodes: {} };
  }
  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) {
      return { version: 2, nodes: {} };
    }
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") {
      return { version: 2, nodes: {} };
    }

    const rawNodes =
      (parsed as { version?: unknown; nodes?: unknown }).version === 2 &&
      (parsed as { nodes?: unknown }).nodes &&
      typeof (parsed as { nodes?: unknown }).nodes === "object"
        ? ((parsed as { nodes: Record<string, unknown> }).nodes)
        : (parsed as Record<string, unknown>);
    const nodes: StoredGraphLayout["nodes"] = {};
    for (const [nodeId, value] of Object.entries(rawNodes)) {
      const rawX = (value as { x?: unknown } | null)?.x;
      const rawY = (value as { y?: unknown } | null)?.y;
      if (
        value &&
        typeof value === "object" &&
        typeof rawX === "number" &&
        typeof rawY === "number" &&
        Number.isFinite(rawX) &&
        Number.isFinite(rawY)
      ) {
        nodes[nodeId] = {
          x: finiteGraphPosition(rawX),
          y: finiteGraphPosition(rawY),
          pinned: true,
          updatedAt:
            typeof (value as { updatedAt?: unknown }).updatedAt === "number"
              ? (value as { updatedAt: number }).updatedAt
              : 0,
        };
      }
    }
    return { version: 2, nodes };
  } catch {
    return { version: 2, nodes: {} };
  }
}

export function persistGraphPositions(
  key: string,
  positions: Record<string, { x: number; y: number }>,
  pinnedNodeIds: Set<string>,
) {
  if (typeof window === "undefined") {
    return;
  }
  try {
    const existing = readStoredGraphLayout(key);
    const next: StoredGraphLayout = {
      version: 2,
      nodes: { ...existing.nodes },
    };
    const updatedAt = Date.now();
    for (const nodeId of pinnedNodeIds) {
      const position = positions[nodeId];
      if (
        position &&
        Number.isFinite(position.x) &&
        Number.isFinite(position.y)
      ) {
        next.nodes[nodeId] = {
          x: position.x,
          y: position.y,
          pinned: true,
          updatedAt,
        };
      }
    }

    if (Object.keys(next.nodes).length === 0) {
      window.localStorage.removeItem(key);
    } else {
      window.localStorage.setItem(key, JSON.stringify(next));
    }
  } catch {
    // Local graph positioning should never block graph interaction.
  }
}
