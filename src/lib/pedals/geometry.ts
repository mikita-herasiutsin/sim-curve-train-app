/**
 * Computes the pixel fill height of a pedal bar from a normalized 0..1 value.
 */
export function getBarFillHeight(value: number, maxBarHeight: number): number {
  const clamped = Math.max(0, Math.min(1, value));
  return clamped * maxBarHeight;
}

/**
 * Returns the rounded integer percentage (0–100) for a normalized 0..1 value.
 */
export function getPercentLabel(value: number): number {
  const clamped = Math.max(0, Math.min(1, value));
  return Math.round(clamped * 100);
}

/**
 * Formats a normalized 0..1 value as a percentage string (e.g. "72%").
 */
export function formatPercent(value: number): string {
  return `${getPercentLabel(value)}%`;
}

/**
 * Calculates the horizontal X coordinate on the canvas for a timestamp.
 * - now is at the right edge (x = width)
 * - older timestamps move to the left
 * - (now - windowUs) is at the left edge (x = 0)
 *
 * @param tUs Monotonic frame timestamp in microseconds
 * @param nowUs Current timestamp in microseconds
 * @param windowUs Time window duration in microseconds
 * @param width Canvas width in pixels
 */
export function getGraphX(tUs: number, nowUs: number, windowUs: number, width: number): number {
  if (windowUs <= 0 || width <= 0) {
    return 0;
  }
  const dtUs = nowUs - tUs;
  return width * (1 - dtUs / windowUs);
}

/**
 * Calculates the vertical Y coordinate on the canvas for a pedal value (0..1).
 * In canvas coordinates, y = 0 is at the top (100% pedal input)
 * and y = height is at the bottom (0% pedal input).
 *
 * @param value Normalized pedal value (0..1)
 * @param height Total canvas height in pixels
 * @param paddingTop Optional top padding in pixels
 * @param paddingBottom Optional bottom padding in pixels
 */
export function getGraphY(
  value: number,
  height: number,
  paddingTop = 0,
  paddingBottom = 0,
): number {
  const clamped = Math.max(0, Math.min(1, value));
  const usableH = Math.max(0, height - paddingTop - paddingBottom);
  return paddingTop + (1 - clamped) * usableH;
}
