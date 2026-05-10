export function nextGraphZoom(currentZoom: number, wheelDeltaY: number): number {
  const direction = wheelDeltaY < 0 ? 1 : -1;
  const factor = direction > 0 ? 1.12 : 1 / 1.12;
  return clamp(currentZoom * factor, 0.45, 3.5);
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

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
