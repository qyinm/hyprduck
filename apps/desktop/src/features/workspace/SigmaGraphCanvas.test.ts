import { describe, expect, test } from "bun:test";

import {
  fitGraphViewportToBounds,
  graphPositionFromPointerDelta,
  nextGraphZoom,
  pointerDeltaToViewBox,
  zoomGraphViewportAtPoint,
} from "./graphViewport";
import {
  sigmaGraphScopeFromUiState,
  sigmaGraphSelectionFromUiState,
  sigmaGraphTopologySelectionFromUiState,
} from "./SigmaGraphCanvas";
import type { WorkspaceUiState } from "./state";

const baseUiState: WorkspaceUiState = {
  selectedNodeId: null,
  selectedEdgeId: null,
  inspectorOpen: true,
  answerDockOpen: false,
  answerInput: "",
};

describe("graph viewport controls", () => {
  test("passes workspace node and edge selection into graph presentation", () => {
    expect(
      sigmaGraphSelectionFromUiState({
        ...baseUiState,
        selectedNodeId: "node-a",
        selectedEdgeId: "edge-a",
      }),
    ).toEqual({
      selectedNodeId: "node-a",
      selectedEdgeId: "edge-a",
    });
  });

  test("keeps selected ids out of global topology inputs", () => {
    const first = {
      ...baseUiState,
      selectedNodeId: "node-a",
      selectedEdgeId: "edge-a",
    };
    const second = {
      ...baseUiState,
      selectedNodeId: "node-b",
      selectedEdgeId: "edge-b",
    };

    expect(sigmaGraphScopeFromUiState("global", first)).toEqual(
      sigmaGraphScopeFromUiState("global", second),
    );
    expect(sigmaGraphTopologySelectionFromUiState(first)).toEqual({
      selectedNodeId: null,
      selectedEdgeId: null,
    });
    expect(sigmaGraphTopologySelectionFromUiState(second)).toEqual({
      selectedNodeId: null,
      selectedEdgeId: null,
    });
  });

  test("keeps local topology scoped around the selected node", () => {
    expect(
      sigmaGraphScopeFromUiState("local", {
        ...baseUiState,
        selectedNodeId: "node-a",
      }),
    ).toEqual({
      mode: "local",
      centerNodeId: "node-a",
      depth: 1,
    });
    expect(sigmaGraphScopeFromUiState("local", baseUiState)).toEqual({
      mode: "local",
      centerNodeId: null,
      depth: 1,
    });
  });

  test("zooms in and out from wheel deltas while staying within bounds", () => {
    const zoomedIn = nextGraphZoom(1, -100);
    expect(zoomedIn).toBeCloseTo(Math.exp(0.25));
    expect(nextGraphZoom(zoomedIn, 100)).toBeCloseTo(1);
    expect(nextGraphZoom(1, 0)).toBe(1);
    expect(nextGraphZoom(6, -100)).toBe(6);
    expect(nextGraphZoom(0.05, 100)).toBe(0.05);
  });

  test("zooms around the pointer position", () => {
    const viewport = { panX: 0, panY: 0, zoom: 1 };
    const point = { x: 30, y: 70 };
    const nextViewport = zoomGraphViewportAtPoint(viewport, -100, point);

    expect(nextViewport.zoom).toBeCloseTo(Math.exp(0.25));
    expect(nextViewport.panX + point.x * nextViewport.zoom).toBeCloseTo(point.x);
    expect(nextViewport.panY + point.y * nextViewport.zoom).toBeCloseTo(point.y);
    expect(zoomGraphViewportAtPoint(viewport, 0, point)).toBe(viewport);
  });

  test("converts pointer movement into zoom-aware viewBox movement", () => {
    expect(pointerDeltaToViewBox(50, 500, 1)).toBeCloseTo(10);
    expect(pointerDeltaToViewBox(50, 500, 2)).toBeCloseTo(5);
    expect(pointerDeltaToViewBox(50, 0, 1)).toBe(0);
  });

  test("converts pointer movement into graph node movement", () => {
    expect(graphPositionFromPointerDelta(50, -50, 500, 500, 1)).toEqual({
      x: 0.2,
      y: 0.2,
    });
    expect(graphPositionFromPointerDelta(50, 50, 500, 500, 2)).toEqual({
      x: 0.1,
      y: -0.1,
    });
  });

  test("fits graph bounds into the viewBox with padding", () => {
    const viewport = fitGraphViewportToBounds({
      minX: -2,
      minY: -1,
      maxX: 2,
      maxY: 1,
    });

    expect(viewport.zoom).toBeGreaterThan(0);
    expect(viewport.zoom).toBeLessThanOrEqual(6);
    expect(Number.isFinite(viewport.panX)).toBe(true);
    expect(Number.isFinite(viewport.panY)).toBe(true);
  });
});
