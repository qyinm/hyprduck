export interface GraphViewport {
  panX: number;
  panY: number;
  zoom: number;
}

export function nextGraphZoom(currentZoom: number, wheelDeltaY: number): number {
  if (wheelDeltaY === 0) {
    return currentZoom;
  }

  const clampedDelta = Math.max(-120, Math.min(120, wheelDeltaY));
  const factor = Math.exp(-clampedDelta * 0.0025);
  return clamp(currentZoom * factor, 0.05, 6);
}

export function zoomGraphViewportAtPoint(
  current: GraphViewport,
  wheelDeltaY: number,
  point: { x: number; y: number },
): GraphViewport {
  const nextZoom = nextGraphZoom(current.zoom, wheelDeltaY);
  if (nextZoom === current.zoom) {
    return current;
  }

  const graphX = (point.x - current.panX) / current.zoom;
  const graphY = (point.y - current.panY) / current.zoom;

  return {
    panX: point.x - graphX * nextZoom,
    panY: point.y - graphY * nextZoom,
    zoom: nextZoom,
  };
}

export function pointerDeltaToViewBox(
  pointerDelta: number,
  viewportSize: number,
  zoom: number,
): number {
  if (viewportSize <= 0 || zoom <= 0) {
    return 0;
  }

  return (pointerDelta / viewportSize) * (100 / zoom);
}

export function graphPositionFromPointerDelta(
  deltaX: number,
  deltaY: number,
  viewportWidth: number,
  viewportHeight: number,
  zoom: number,
): { x: number; y: number } {
  return {
    x: pointerDeltaToViewBox(deltaX, viewportWidth, zoom) / 50,
    y: -pointerDeltaToViewBox(deltaY, viewportHeight, zoom) / 50,
  };
}

export function fitGraphViewportToBounds(
  bounds: { minX: number; minY: number; maxX: number; maxY: number },
  padding = 8,
): GraphViewport {
  const minViewX = bounds.minX * 50 + 50;
  const maxViewX = bounds.maxX * 50 + 50;
  const minViewY = 50 - bounds.maxY * 50;
  const maxViewY = 50 - bounds.minY * 50;
  const width = Math.max(1, maxViewX - minViewX + padding * 2);
  const height = Math.max(1, maxViewY - minViewY + padding * 2);
  const zoom = clamp(Math.min(100 / width, 100 / height), 0.05, 6);
  const centerX = (minViewX + maxViewX) / 2;
  const centerY = (minViewY + maxViewY) / 2;

  return {
    panX: 50 - centerX * zoom,
    panY: 50 - centerY * zoom,
    zoom,
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
