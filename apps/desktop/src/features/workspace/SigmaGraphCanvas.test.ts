import { describe, expect, test } from "bun:test";

import {
  graphPositionFromPointerDelta,
  nextGraphZoom,
  pointerDeltaToViewBox,
  zoomGraphViewportAtPoint,
} from "./graphViewport";

describe("graph viewport controls", () => {
  test("zooms in and out from wheel deltas while staying within bounds", () => {
    expect(nextGraphZoom(1, -100)).toBeCloseTo(1.12);
    expect(nextGraphZoom(1.12, 100)).toBeCloseTo(1);
    expect(nextGraphZoom(1, 0)).toBe(1);
    expect(nextGraphZoom(4, -100)).toBe(3.5);
    expect(nextGraphZoom(0.1, 100)).toBe(0.45);
  });

  test("zooms around the pointer position", () => {
    const viewport = { panX: 0, panY: 0, zoom: 1 };
    const point = { x: 30, y: 70 };
    const nextViewport = zoomGraphViewportAtPoint(viewport, -100, point);

    expect(nextViewport.zoom).toBeCloseTo(1.12);
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
});
