import { bandColor } from "./barsDraw";
import { getGraphY } from "./geometry";
import { computeHorizontalGridLines } from "./graphDraw";
import type { AppThemeColors } from "./theme";

/** Timing window (ms) of the band, mirroring RAMP_WINDOW_MS in crates/core/src/trace_scoring.rs. */
export const TRACE_RAMP_WINDOW_MS = 150;

export class TraceCurve {
  readonly points: [number, number][];
  readonly durationMs: number;
  private readonly m: number[];
  private readonly minArr: Float64Array;
  private readonly maxArr: Float64Array;

  constructor(points: [number, number][]) {
    this.points = points.slice();
    this.durationMs = traceDurationMs(this.points);

    const n = this.points.length;
    if (n <= 1) {
      this.m = [];
      this.minArr = new Float64Array(0);
      this.maxArr = new Float64Array(0);
      return;
    }

    const d = new Float64Array(n - 1);
    for (let k = 0; k < n - 1; k++) {
      const dt = this.points[k + 1][0] - this.points[k][0];
      d[k] = dt === 0 ? 0 : (this.points[k + 1][1] - this.points[k][1]) / dt;
    }

    const m = new Float64Array(n);
    m[0] = 0;
    m[n - 1] = 0;
    for (let k = 1; k < n - 1; k++) {
      if (d[k - 1] * d[k] <= 0) {
        m[k] = 0;
      } else {
        m[k] = (d[k - 1] + d[k]) / 2;
      }
    }

    for (let k = 0; k < n - 1; k++) {
      if (d[k] === 0) {
        m[k] = 0;
        m[k + 1] = 0;
        continue;
      }
      const a = m[k] / d[k];
      const b = m[k + 1] / d[k];
      if (a * a + b * b > 9) {
        const tau = 3 / Math.hypot(a, b);
        m[k] = tau * a * d[k];
        m[k + 1] = tau * b * d[k];
      }
    }
    this.m = Array.from(m);

    // Envelope on a 1 ms grid over [-window, duration + window]. Further out every value in the
    // window is the clamped first or last value, so envelopeAt falls back to valueAt there.
    const window = TRACE_RAMP_WINDOW_MS;
    const len = Math.max(0, Math.ceil(this.durationMs)) + 2 * window + 1;
    const grid = new Float64Array(len);
    for (let i = 0; i < len; i++) {
      grid[i] = this.valueAt(i - window);
    }
    this.minArr = new Float64Array(len);
    this.maxArr = new Float64Array(len);
    for (let i = 0; i < len; i++) {
      let lo = Infinity;
      let hi = -Infinity;
      for (let j = Math.max(0, i - window); j <= Math.min(len - 1, i + window); j++) {
        lo = Math.min(lo, grid[j]);
        hi = Math.max(hi, grid[j]);
      }
      this.minArr[i] = lo;
      this.maxArr[i] = hi;
    }
  }

  /** Target fraction 0..1 at tMs (monotone cubic, see below). */
  valueAt(tMs: number): number {
    const n = this.points.length;
    if (n === 0) {
      return 0;
    }
    if (n === 1) {
      return this.points[0][1] / 100;
    }
    if (!Number.isFinite(tMs) || tMs <= this.points[0][0]) {
      return this.points[0][1] / 100;
    }
    if (tMs >= this.points[n - 1][0]) {
      return this.points[n - 1][1] / 100;
    }

    let low = 0;
    let high = n;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (this.points[mid][0] <= tMs) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    const k = low - 1;
    const [t0, y0] = this.points[k];
    const [t1, y1] = this.points[k + 1];
    const h = t1 - t0;
    if (h <= 0) {
      return y0 / 100;
    }
    const mk = this.m[k];
    const mk1 = this.m[k + 1];
    if (y0 === y1 && mk === 0 && mk1 === 0) {
      return y0 / 100;
    }
    const s = (tMs - t0) / h;
    const s2 = s * s;
    const s3 = s2 * s;
    const v =
      (2 * s3 - 3 * s2 + 1) * y0 +
      (s3 - 2 * s2 + s) * h * mk +
      (-2 * s3 + 3 * s2) * y1 +
      (s3 - s2) * h * mk1;
    return v / 100;
  }

  /** [min, max] of valueAt over integer ms offsets -window..+window around round(tMs); fractions. */
  envelopeAt(tMs: number): [number, number] {
    const i = Math.round(tMs) + TRACE_RAMP_WINDOW_MS;
    if (!(i >= 0 && i < this.minArr.length)) {
      const v = this.valueAt(tMs);
      return [v, v];
    }
    return [this.minArr[i], this.maxArr[i]];
  }
}

/** Rep duration in ms: time of the last point, 0 when empty. */
export function traceDurationMs(points: [number, number][]): number {
  if (points.length === 0) return 0;
  return points[points.length - 1][0];
}

/** The approach second before the rep starts. */
export const GO_LEAD_MS = 1000;

/** X pixel for rep time tMs on a plot spanning [startMs, durationMs] across [padLeft, width - padRight]. Not clamped. */
export function traceX(
  tMs: number,
  durationMs: number,
  width: number,
  padLeft: number,
  padRight: number,
  startMs = 0,
): number {
  const usableW = width - padLeft - padRight;
  const totalDuration = durationMs - startMs;
  if (totalDuration <= 0 || usableW <= 0) {
    return padLeft;
  }
  return padLeft + ((tMs - startMs) / totalDuration) * usableW;
}

export interface TraceViewState {
  curve: TraceCurve;
  pedal: "brake" | "throttle";
  tolerance: number; // fraction 0..1
  /** Rep time of the playhead in ms, or null to hide it (idle, countdown of the first rep). */
  playheadMs: number | null;
  /** The user's frames for the rep shown, as [repTimeMs, value 0..1], oldest first. */
  user: [number, number][];
  /** Whether the user's latest value is inside the band at the playhead (colours the band). */
  inBand: boolean;
}

/** Draws background, horizontal grid (reuse computeHorizontalGridLines and the label style of drawGraph),
 *  vertical grid lines every 500 ms labelled in seconds ("0.5 s"), the tolerance band as a filled polygon
 *  (curve + tol on top, curve - tol on the bottom, both clamped to 0..1, colour bandColor(inBand, theme)),
 *  the target curve as a 2 px dashed line in theme.text, the user's trace as a 2.5 px solid line in the
 *  pedal colour (theme.brake / theme.throttle), and the playhead as a 2 px vertical line in theme.accent. */
export function drawTrace(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  state: TraceViewState,
  theme: AppThemeColors,
  paddingTop = 18,
  paddingBottom = 26,
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
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";

  for (const line of hLines) {
    ctx.setLineDash(line.boundary ? [] : [4, 4]);
    ctx.strokeStyle = line.boundary ? theme.textMuted : theme.border;
    ctx.beginPath();
    ctx.moveTo(0, line.y);
    ctx.lineTo(width, line.y);
    ctx.stroke();

    ctx.fillStyle = theme.textMuted;
    // The 100% label goes below its line so it stays inside the plot.
    ctx.fillText(line.label, 8, line.fraction === 1 ? line.y + 9 : line.y - 7);
  }

  const durationMs = state.curve.durationMs;

  // Vertical grid lines and rep start mark
  if (durationMs > 0 && width > 0) {
    // A thin solid vertical line in theme.textMuted at t = 0 marks where the rep starts
    const x0 = traceX(0, durationMs, width, 0, 0, -GO_LEAD_MS);
    ctx.strokeStyle = theme.textMuted;
    ctx.lineWidth = 1;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.moveTo(x0, paddingTop);
    ctx.lineTo(x0, height - paddingBottom);
    ctx.stroke();

    // Vertical grid lines every 500 ms from 0 (labels "0.5 s" etc. as today)
    ctx.strokeStyle = theme.border;
    ctx.setLineDash([4, 4]);
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    ctx.fillStyle = theme.textMuted;

    for (let tMs = 500; tMs <= durationMs; tMs += 500) {
      const x = traceX(tMs, durationMs, width, 0, 0, -GO_LEAD_MS);
      if (x < 0 || x > width) continue;
      ctx.beginPath();
      ctx.moveTo(x, paddingTop);
      ctx.lineTo(x, height - paddingBottom);
      ctx.stroke();

      if (x >= 16 && x <= width - 16) {
        ctx.fillText(`${tMs / 1000} s`, x, height - 6);
      }
    }
    ctx.setLineDash([]);
  }

  // Tolerance band as a filled polygon
  // Band polygon: upper min(1, hi + tolerance), lower max(0, lo - tolerance) from envelopeAt,
  // sampled every 10 ms across the domain plus at each point time.
  if (state.curve.points.length >= 2 && durationMs > 0) {
    const timeSet = new Set<number>();
    for (const p of state.curve.points) {
      timeSet.add(p[0]);
    }
    for (let t = -GO_LEAD_MS; t <= durationMs; t += 10) {
      timeSet.add(t);
    }
    timeSet.add(-GO_LEAD_MS);
    timeSet.add(0);
    timeSet.add(durationMs);
    const sampleTimes = Array.from(timeSet).sort((a, b) => a - b);

    ctx.fillStyle = bandColor(state.inBand, theme);
    ctx.beginPath();
    for (let i = 0; i < sampleTimes.length; i++) {
      const t = sampleTimes[i];
      const [, hi] = state.curve.envelopeAt(t);
      const topVal = Math.min(1, hi + state.tolerance);
      const x = traceX(t, durationMs, width, 0, 0, -GO_LEAD_MS);
      const y = getGraphY(topVal, height, paddingTop, paddingBottom);
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    }
    for (let i = sampleTimes.length - 1; i >= 0; i--) {
      const t = sampleTimes[i];
      const [lo] = state.curve.envelopeAt(t);
      const botVal = Math.max(0, lo - state.tolerance);
      const x = traceX(t, durationMs, width, 0, 0, -GO_LEAD_MS);
      const y = getGraphY(botVal, height, paddingTop, paddingBottom);
      ctx.lineTo(x, y);
    }
    ctx.closePath();
    ctx.fill();
  }

  // Target curve: dashed line sampled from valueAt every 5 ms across the domain
  if (state.curve.points.length > 0 && durationMs > 0) {
    ctx.strokeStyle = theme.text;
    ctx.lineWidth = 2;
    ctx.setLineDash([4, 4]);
    ctx.beginPath();
    let first = true;
    for (let t = -GO_LEAD_MS; t <= durationMs; t += 5) {
      const val = state.curve.valueAt(t);
      const x = traceX(t, durationMs, width, 0, 0, -GO_LEAD_MS);
      const y = getGraphY(val, height, paddingTop, paddingBottom);
      if (first) {
        ctx.moveTo(x, y);
        first = false;
      } else {
        ctx.lineTo(x, y);
      }
    }
    if (durationMs % 5 !== 0) {
      const val = state.curve.valueAt(durationMs);
      const x = traceX(durationMs, durationMs, width, 0, 0, -GO_LEAD_MS);
      const y = getGraphY(val, height, paddingTop, paddingBottom);
      ctx.lineTo(x, y);
    }
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // User trace as a 2.5 px solid line in pedal colour
  if (state.user.length > 0 && durationMs > 0) {
    ctx.strokeStyle = state.pedal === "brake" ? theme.brake : theme.throttle;
    ctx.lineWidth = 2.5;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.setLineDash([]);
    ctx.beginPath();
    for (let i = 0; i < state.user.length; i++) {
      const [userT, userVal] = state.user[i];
      const x = traceX(userT, durationMs, width, 0, 0, -GO_LEAD_MS);
      const y = getGraphY(userVal, height, paddingTop, paddingBottom);
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    }
    ctx.stroke();
  }

  // Playhead as a 2 px vertical line in theme.accent
  if (state.playheadMs !== null && durationMs > 0) {
    const x = traceX(state.playheadMs, durationMs, width, 0, 0, -GO_LEAD_MS);
    ctx.strokeStyle = theme.accent;
    ctx.lineWidth = 2;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.moveTo(x, paddingTop);
    ctx.lineTo(x, height - paddingBottom);
    ctx.stroke();
  }
}
