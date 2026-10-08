import { getGraphX, getGraphY } from "./geometry";
import type { AppThemeColors } from "./theme";

export interface VerticalGridLine {
  x: number;
  label: string;
}

export interface HorizontalGridLine {
  y: number;
  fraction: number;
  label: string;
  /** 0% and 100% are solid boundaries, so a full or released pedal visibly sits on them. */
  boundary: boolean;
}

export interface Point {
  x: number;
  y: number;
}

export class ColumnDecimator {
  numCols: number;
  hasData: Uint8Array;
  brakeMin: Float32Array;
  brakeMax: Float32Array;
  throttleMin: Float32Array;
  throttleMax: Float32Array;

  constructor(numCols: number) {
    this.numCols = Math.max(1, numCols);
    this.hasData = new Uint8Array(this.numCols);
    this.brakeMin = new Float32Array(this.numCols);
    this.brakeMax = new Float32Array(this.numCols);
    this.throttleMin = new Float32Array(this.numCols);
    this.throttleMax = new Float32Array(this.numCols);
  }

  resize(numCols: number): void {
    const cols = Math.max(1, Math.floor(numCols));
    if (cols === this.numCols) {
      return;
    }
    this.numCols = cols;
    this.hasData = new Uint8Array(cols);
    this.brakeMin = new Float32Array(cols);
    this.brakeMax = new Float32Array(cols);
    this.throttleMin = new Float32Array(cols);
    this.throttleMax = new Float32Array(cols);
  }

  reset(): void {
    this.hasData.fill(0);
    this.brakeMin.fill(1);
    this.brakeMax.fill(0);
    this.throttleMin.fill(1);
    this.throttleMax.fill(0);
  }

  accumulate(col: number, brake: number, throttle: number): void {
    if (col < 0 || col >= this.numCols) {
      return;
    }

    if (this.hasData[col] === 0) {
      this.hasData[col] = 1;
      this.brakeMin[col] = brake;
      this.brakeMax[col] = brake;
      this.throttleMin[col] = throttle;
      this.throttleMax[col] = throttle;
    } else {
      if (brake < this.brakeMin[col]) this.brakeMin[col] = brake;
      if (brake > this.brakeMax[col]) this.brakeMax[col] = brake;
      if (throttle < this.throttleMin[col]) this.throttleMin[col] = throttle;
      if (throttle > this.throttleMax[col]) this.throttleMax[col] = throttle;
    }
  }
}

/**
 * Computes horizontal grid lines at 0%, 25%, 50%, 75% and 100%.
 */
export function computeHorizontalGridLines(
  height: number,
  paddingTop = 16,
  paddingBottom = 24,
): HorizontalGridLine[] {
  return [0, 0.25, 0.5, 0.75, 1].map((fraction) => ({
    y: getGraphY(fraction, height, paddingTop, paddingBottom),
    fraction,
    label: `${Math.round(fraction * 100)}%`,
    boundary: fraction === 0 || fraction === 1,
  }));
}

/**
 * Computes vertical grid lines at every 1-second interval scrolling with the graph.
 */
export function computeVerticalGridLines(
  nowUs: number,
  windowUs: number,
  width: number,
): VerticalGridLine[] {
  if (windowUs <= 0 || width <= 0) {
    return [];
  }

  const lines: VerticalGridLine[] = [];
  const latestSecondUs = Math.floor(nowUs / 1_000_000) * 1_000_000;
  const minTUs = nowUs - windowUs;

  for (let t = latestSecondUs; t >= minTUs; t -= 1_000_000) {
    const x = getGraphX(t, nowUs, windowUs, width);
    const diffSec = Math.round((t - nowUs) / 1_000_000);
    lines.push({
      x,
      label: diffSec === 0 ? "now" : `${diffSec}s`,
    });
  }

  return lines;
}

/**
 * Builds continuous segments of points for a signal from decimated columns.
 * If min !== max in a column, both values are included so spikes are not lost.
 */
export function buildSignalSegments(
  hasData: Uint8Array,
  minVals: Float32Array,
  maxVals: Float32Array,
  numCols: number,
  height: number,
  paddingTop = 16,
  paddingBottom = 24,
  maxColGap = 4,
  dpr = 1,
): Point[][] {
  const segments: Point[][] = [];
  let currentSegment: Point[] = [];
  let lastColWithData = -1;

  for (let col = 0; col < numCols; col++) {
    if (hasData[col] === 0) {
      continue;
    }

    if (lastColWithData !== -1 && col - lastColWithData > maxColGap) {
      if (currentSegment.length > 0) {
        segments.push(currentSegment);
        currentSegment = [];
      }
    }

    const x = col / dpr;
    const yMin = getGraphY(minVals[col], height, paddingTop, paddingBottom);
    const yMax = getGraphY(maxVals[col], height, paddingTop, paddingBottom);

    if (Math.abs(yMin - yMax) < 0.5) {
      currentSegment.push({ x, y: yMin });
    } else {
      // In canvas coordinates, higher pedal value has lower Y
      // Order points to keep line smooth
      const lastPoint = currentSegment[currentSegment.length - 1];
      if (!lastPoint || Math.abs(lastPoint.y - yMin) <= Math.abs(lastPoint.y - yMax)) {
        currentSegment.push({ x, y: yMin });
        currentSegment.push({ x, y: yMax });
      } else {
        currentSegment.push({ x, y: yMax });
        currentSegment.push({ x, y: yMin });
      }
    }

    lastColWithData = col;
  }

  if (currentSegment.length > 0) {
    segments.push(currentSegment);
  }

  return segments;
}

/** A target band (target +- tolerance, as 0..1 fractions) for one pedal. */
export interface TargetBand {
  pedal: "brake" | "throttle" | "clutch";
  target: number;
  tolerance: number;
}

/** Shades the target band across the whole graph, in the pedal's colour. */
export function drawTargetBand(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  band: TargetBand,
  theme: AppThemeColors,
  paddingTop: number,
  paddingBottom: number,
): void {
  const top = getGraphY(
    Math.min(1, band.target + band.tolerance),
    height,
    paddingTop,
    paddingBottom,
  );
  const bottom = getGraphY(
    Math.max(0, band.target - band.tolerance),
    height,
    paddingTop,
    paddingBottom,
  );
  ctx.fillStyle = (band.pedal === "brake" ? theme.brake : theme.throttle) + "30";
  ctx.fillRect(0, top, width, bottom - top);
}

/**
 * Draws the complete pedal graph onto a CanvasRenderingContext2D.
 */
export function drawGraph(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  decimator: ColumnDecimator,
  nowUs: number,
  windowUs: number,
  theme: AppThemeColors,
  paddingTop = 18,
  paddingBottom = 26,
  dpr = 1,
  band: TargetBand | null = null,
): void {
  // Clear canvas
  ctx.clearRect(0, 0, width, height);

  // Background
  ctx.fillStyle = theme.surface;
  ctx.fillRect(0, 0, width, height);

  // Grid lines styling
  ctx.font = '11px "Inter", system-ui, sans-serif';
  ctx.lineWidth = 1;

  // Horizontal grid lines: solid 0% and 100% boundaries, dashed 25%, 50%, 75%
  const hLines = computeHorizontalGridLines(height, paddingTop, paddingBottom);
  ctx.strokeStyle = theme.border;
  ctx.fillStyle = theme.textMuted;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";

  for (const line of hLines) {
    ctx.setLineDash(line.boundary ? [] : [4, 4]);
    ctx.strokeStyle = line.boundary ? theme.textMuted : theme.border;
    ctx.beginPath();
    ctx.moveTo(0, line.y);
    ctx.lineTo(width, line.y);
    ctx.stroke();

    // The 100% label goes below its line so it stays inside the plot.
    ctx.fillText(line.label, 8, line.fraction === 1 ? line.y + 9 : line.y - 7);
  }
  ctx.strokeStyle = theme.border;
  ctx.setLineDash([4, 4]);

  // Vertical grid lines (every 1 second)
  const vLines = computeVerticalGridLines(nowUs, windowUs, width);
  ctx.textAlign = "center";
  ctx.textBaseline = "bottom";

  for (const line of vLines) {
    if (line.x < 0 || line.x > width) continue;
    ctx.beginPath();
    ctx.moveTo(line.x, paddingTop);
    ctx.lineTo(line.x, height - paddingBottom);
    ctx.stroke();

    // Centred labels at the very edges would be clipped.
    if (line.x >= 16 && line.x <= width - 16) ctx.fillText(line.label, line.x, height - 6);
  }
  ctx.setLineDash([]);

  if (band) drawTargetBand(ctx, width, height, band, theme, paddingTop, paddingBottom);

  // Draw Brake line
  const brakeSegments = buildSignalSegments(
    decimator.hasData,
    decimator.brakeMin,
    decimator.brakeMax,
    decimator.numCols,
    height,
    paddingTop,
    paddingBottom,
    Math.max(4, Math.round(4 * dpr)),
    dpr,
  );

  ctx.strokeStyle = theme.brake;
  ctx.lineWidth = 2.5;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";

  for (const segment of brakeSegments) {
    if (segment.length === 0) continue;
    ctx.beginPath();
    ctx.moveTo(segment[0].x, segment[0].y);
    for (let i = 1; i < segment.length; i++) {
      ctx.lineTo(segment[i].x, segment[i].y);
    }
    ctx.stroke();
  }

  // Draw Throttle line
  const throttleSegments = buildSignalSegments(
    decimator.hasData,
    decimator.throttleMin,
    decimator.throttleMax,
    decimator.numCols,
    height,
    paddingTop,
    paddingBottom,
    Math.max(4, Math.round(4 * dpr)),
    dpr,
  );

  ctx.strokeStyle = theme.throttle;
  ctx.lineWidth = 2.5;

  for (const segment of throttleSegments) {
    if (segment.length === 0) continue;
    ctx.beginPath();
    ctx.moveTo(segment[0].x, segment[0].y);
    for (let i = 1; i < segment.length; i++) {
      ctx.lineTo(segment[i].x, segment[i].y);
    }
    ctx.stroke();
  }
}
