export interface GraphViewport {
  panX: number;
  panY: number;
  zoom: number;
}

export function nextGraphZoom(currentZoom: number, wheelDeltaY: number): number {
  if (wheelDeltaY === 0) {
    return currentZoom;
  }

  const direction = wheelDeltaY < 0 ? 1 : -1;
  const factor = direction > 0 ? 1.12 : 1 / 1.12;
  return clamp(currentZoom * factor, 0.45, 3.5);
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

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
