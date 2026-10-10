import { bandColor } from "./barsDraw";
import { formatPercentValue, getGraphY } from "./geometry";
import { computeHorizontalGridLines } from "./graphDraw";
import type { AppThemeColors } from "./theme";

/** Timing window (ms) of the band, mirroring RAMP_WINDOW_MS in crates/core/src/trace_scoring.rs. */
export const TRACE_RAMP_WINDOW_MS = 150;

export class TraceCurve {
  readonly points: [number, number][];
  readonly durationMs: number;
  readonly sampleTimes: number[];
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
      this.sampleTimes = [];
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

    const minDeque = new Int32Array(len);
    let minHead = 0;
    let minTail = 0;

    const maxDeque = new Int32Array(len);
    let maxHead = 0;
    let maxTail = 0;

    let right = -1;
    for (let i = 0; i < len; i++) {
      const targetR = Math.min(len - 1, i + window);
      while (right < targetR) {
        right++;
        const val = grid[right];
        while (minTail > minHead && grid[minDeque[minTail - 1]] >= val) {
          minTail--;
        }
        minDeque[minTail++] = right;

        while (maxTail > maxHead && grid[maxDeque[maxTail - 1]] <= val) {
          maxTail--;
        }
        maxDeque[maxTail++] = right;
      }

      const targetL = Math.max(0, i - window);
      while (minHead < minTail && minDeque[minHead] < targetL) {
        minHead++;
      }
      while (maxHead < maxTail && maxDeque[maxHead] < targetL) {
        maxHead++;
      }

      this.minArr[i] = grid[minDeque[minHead]];
      this.maxArr[i] = grid[maxDeque[maxHead]];
    }

    const timeSet = new Set<number>();
    for (const p of this.points) {
      timeSet.add(p[0]);
    }
    for (let t = -GO_LEAD_MS; t <= this.durationMs; t += 10) {
      timeSet.add(t);
    }
    timeSet.add(-GO_LEAD_MS);
    timeSet.add(0);
    timeSet.add(this.durationMs);
    this.sampleTimes = Array.from(timeSet).sort((a, b) => a - b);
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
  /** Number of decimals for the target % label at the now-line (default 0). */
  decimals?: number;
}

export interface TracePhaseInput {
  nowUs: number;
  countingDown: boolean;
  countdownEndsUs: number;
  active: boolean;
  repStartUs: number;
  lastShownStartUs: number;
  durationMs: number;
}

export interface TracePhase {
  shownStartUs: number;
  playheadMs: number | null;
}

export function traceViewPhase(i: TracePhaseInput): TracePhase {
  if (i.countingDown) {
    if (i.countdownEndsUs > 0 && i.countdownEndsUs - i.nowUs <= GO_LEAD_MS * 1000) {
      return {
        shownStartUs: i.countdownEndsUs,
        playheadMs: (i.nowUs - i.countdownEndsUs) / 1000,
      };
    }
    return { shownStartUs: 0, playheadMs: null };
  }

  if (i.active) {
    const repMs = (i.nowUs - i.repStartUs) / 1000;
    return {
      shownStartUs: i.repStartUs,
      playheadMs: Math.max(-GO_LEAD_MS, Math.min(i.durationMs, repMs)),
    };
  }

  return {
    shownStartUs: i.lastShownStartUs > 0 ? i.lastShownStartUs : i.repStartUs,
    playheadMs: null,
  };
}

export type TimeToX = (tMs: number) => number;
export type ValueToY = (fraction: number) => number;

/** Fills the ±150 ms envelope band (D-21): upper min(1, hi + tolerance), lower max(0, lo - tolerance) from curve.envelopeAt(t), at the times times[from..to) (ascending, half-open). Sets ctx.fillStyle = color. No-op when to - from < 2. */
export function drawTraceBand(
  ctx: CanvasRenderingContext2D,
  curve: TraceCurve,
  tolerance: number,
  times: number[],
  color: string,
  xOf: TimeToX,
  yOf: ValueToY,
  from = 0,
  to = times.length,
): void {
  if (to - from < 2) return;

  ctx.fillStyle = color;
  ctx.beginPath();
  for (let i = from; i < to; i++) {
    const t = times[i];
    const [, hi] = curve.envelopeAt(t);
    const topVal = Math.min(1, hi + tolerance);
    const x = xOf(t);
    const y = yOf(topVal);
    if (i === from) {
      ctx.moveTo(x, y);
    } else {
      ctx.lineTo(x, y);
    }
  }
  for (let i = to - 1; i >= from; i--) {
    const t = times[i];
    const [lo] = curve.envelopeAt(t);
    const botVal = Math.max(0, lo - tolerance);
    const x = xOf(t);
    const y = yOf(botVal);
    ctx.lineTo(x, y);
  }
  ctx.closePath();
  ctx.fill();
}

/** Dashed (4,4) 2 px line in `color` of curve.valueAt sampled every 5 ms from fromMs to toMs, plus a final sample at toMs when (toMs - fromMs) % 5 !== 0. dashOffset shifts the dash phase in px. Resets line dash and dash offset after. */
export function drawTargetLine(
  ctx: CanvasRenderingContext2D,
  curve: TraceCurve,
  fromMs: number,
  toMs: number,
  color: string,
  xOf: TimeToX,
  yOf: ValueToY,
  dashOffset = 0,
): void {
  if (fromMs >= toMs) return;

  ctx.strokeStyle = color;
  ctx.lineWidth = 2;
  ctx.setLineDash([4, 4]);
  ctx.beginPath();
  let first = true;
  for (let t = fromMs; t <= toMs; t += 5) {
    const val = curve.valueAt(t);
    const x = xOf(t);
    const y = yOf(val);
    if (first) {
      ctx.moveTo(x, y);
      first = false;
    } else {
      ctx.lineTo(x, y);
    }
  }
  if ((toMs - fromMs) % 5 !== 0) {
    const val = curve.valueAt(toMs);
    const x = xOf(toMs);
    const y = yOf(val);
    ctx.lineTo(x, y);
  }
  ctx.lineDashOffset = dashOffset;
  ctx.stroke();
  ctx.lineDashOffset = 0;
  ctx.setLineDash([]);
}

/** Solid 2.5 px round-joined, round-capped line in `color` through user points [tMs, fraction], starting at index `from`. No-op when none remain. */
export function drawUserLine(
  ctx: CanvasRenderingContext2D,
  user: [number, number][],
  color: string,
  xOf: TimeToX,
  yOf: ValueToY,
  from = 0,
): void {
  if (user.length <= from) return;

  ctx.strokeStyle = color;
  ctx.lineWidth = 2.5;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.setLineDash([]);
  ctx.beginPath();
  for (let i = from; i < user.length; i++) {
    const [userT, userVal] = user[i];
    const x = xOf(userT);
    const y = yOf(userVal);
    if (i === from) {
      ctx.moveTo(x, y);
    } else {
      ctx.lineTo(x, y);
    }
  }
  ctx.stroke();
}

/** Rep-time ms visible left of the now-line. */
export const GHOST_PAST_MS = 1000;
/** Rep-time ms visible right of the now-line. */
export const GHOST_FUTURE_MS = 3000;
/** The now-line sits at this fraction of the width. Define it as GHOST_PAST_MS / (GHOST_PAST_MS + GHOST_FUTURE_MS), which is 0.25. */
export const GHOST_NOW_FRAC: number = GHOST_PAST_MS / (GHOST_PAST_MS + GHOST_FUTURE_MS);

/** X of the now-line: width * GHOST_NOW_FRAC. */
export function ghostNowX(width: number): number {
  return width * GHOST_NOW_FRAC;
}

/** X pixel for rep time tMs when the now-line shows rep time nowMs. Not clamped.
 * ghostNowX(width) + (tMs - nowMs) * width / (GHOST_PAST_MS + GHOST_FUTURE_MS). Returns 0 when width <= 0. */
export function ghostX(tMs: number, nowMs: number, width: number): number {
  if (width <= 0) return 0;
  return ghostNowX(width) + ((tMs - nowMs) * width) / (GHOST_PAST_MS + GHOST_FUTURE_MS);
}

/** Rep time shown at the now-line: playheadMs when not null, otherwise -GO_LEAD_MS (idle before the first rep, the curve waits right of the line). */
export function ghostNowMs(playheadMs: number | null): number {
  return playheadMs ?? -GO_LEAD_MS;
}

/** First index in [0, n) whose time is >= t, or n when none is. timeAt(i) returns the time of index i; times must ascend. */
function lowerBound(n: number, timeAt: (i: number) => number, t: number): number {
  let low = 0;
  let high = n;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (timeAt(mid) < t) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  return low;
}

function drawGridBackground(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  theme: AppThemeColors,
  paddingTop: number,
  paddingBottom: number,
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
}

/**
 * Draws the t=0 rep-start line (solid, theme.textMuted), the dashed 500 ms grid with "0.5 s"
 * labels, and optionally the solid rep-end line at durationMs. xOf maps rep time to x. Lines
 * outside [0, width] and labels outside [16, width - 16] are skipped. No-op when durationMs or
 * width is not positive.
 */
function drawTimeGrid(
  ctx: CanvasRenderingContext2D,
  durationMs: number,
  width: number,
  height: number,
  xOf: TimeToX,
  theme: AppThemeColors,
  paddingTop: number,
  paddingBottom: number,
  drawEndLine: boolean,
): void {
  if (!(durationMs > 0 && width > 0)) return;

  const x0 = xOf(0);
  if (x0 >= 0 && x0 <= width) {
    ctx.strokeStyle = theme.textMuted;
    ctx.lineWidth = 1;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.moveTo(x0, paddingTop);
    ctx.lineTo(x0, height - paddingBottom);
    ctx.stroke();
  }

  ctx.strokeStyle = theme.border;
  ctx.lineWidth = 1;
  ctx.setLineDash([4, 4]);
  ctx.textAlign = "center";
  ctx.textBaseline = "bottom";
  ctx.fillStyle = theme.textMuted;

  for (let tMs = 500; tMs <= durationMs; tMs += 500) {
    const x = xOf(tMs);
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

  if (drawEndLine) {
    const xEnd = xOf(durationMs);
    if (xEnd >= 0 && xEnd <= width) {
      ctx.strokeStyle = theme.textMuted;
      ctx.lineWidth = 1;
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.moveTo(xEnd, paddingTop);
      ctx.lineTo(xEnd, height - paddingBottom);
      ctx.stroke();
    }
  }
}

/**
 * Draws the trace drill view: time axis from -GO_LEAD_MS to the duration with grid lines,
 * the t=0 line marking rep start, the envelope tolerance band, the target curve,
 * the user line, and the playhead.
 */
export function drawTrace(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  state: TraceViewState,
  theme: AppThemeColors,
  paddingTop = 18,
  paddingBottom = 26,
): void {
  drawGridBackground(ctx, width, height, theme, paddingTop, paddingBottom);

  const durationMs = state.curve.durationMs;
  const xOf: TimeToX = (t) => traceX(t, durationMs, width, 0, 0, -GO_LEAD_MS);
  const yOf: ValueToY = (v) => getGraphY(v, height, paddingTop, paddingBottom);

  drawTimeGrid(ctx, durationMs, width, height, xOf, theme, paddingTop, paddingBottom, false);

  // Tolerance band as a filled polygon
  if (state.curve.points.length >= 2 && durationMs > 0) {
    drawTraceBand(
      ctx,
      state.curve,
      state.tolerance,
      state.curve.sampleTimes,
      bandColor(state.inBand, theme),
      xOf,
      yOf,
    );
  }

  // Target curve: dashed line sampled from valueAt every 5 ms across the domain
  if (state.curve.points.length > 0 && durationMs > 0) {
    drawTargetLine(ctx, state.curve, -GO_LEAD_MS, durationMs, theme.text, xOf, yOf);
  }

  // User trace as a 2.5 px solid line in pedal colour
  if (state.user.length > 0 && durationMs > 0) {
    const color = state.pedal === "brake" ? theme.brake : theme.throttle;
    drawUserLine(ctx, state.user, color, xOf, yOf);
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

/**
 * Draws the scrolling ghost trace view: curve scrolls right-to-left towards a fixed
 * vertical now-line, with the target percentage drawn at the line. After a rep (playheadMs
 * null and user frames present) it draws the whole rep the way drawTrace does.
 */
export function drawGhostTrace(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  state: TraceViewState,
  theme: AppThemeColors,
  paddingTop = 18,
  paddingBottom = 26,
): void {
  // A rep is over: show the whole rep, the same picture as the playhead view.
  if (state.playheadMs === null && state.user.length > 0) {
    drawTrace(ctx, width, height, state, theme, paddingTop, paddingBottom);
    return;
  }

  // 1. clearRect + theme.surface background, horizontal grid lines and % labels
  drawGridBackground(ctx, width, height, theme, paddingTop, paddingBottom);

  const durationMs = state.curve.durationMs;

  // 2. nowMs and coordinate mapping functions
  const nowMs = ghostNowMs(state.playheadMs);
  const xOf: TimeToX = (t) => ghostX(t, nowMs, width);
  const yOf: ValueToY = (v) => getGraphY(v, height, paddingTop, paddingBottom);

  // 3. When durationMs > 0: solid t=0 line, dashed 500 ms grid lines, solid t=durationMs line
  drawTimeGrid(ctx, durationMs, width, height, xOf, theme, paddingTop, paddingBottom, true);

  const hasPoints = state.curve.points.length > 0 && durationMs > 0;

  // 4. Band via drawTraceBand, over the sample times in [minBandT, maxBandT]
  if (state.curve.points.length >= 2 && durationMs > 0) {
    const sampleTimes = state.curve.sampleTimes;
    const minBandT = nowMs - GHOST_PAST_MS - 10;
    const maxBandT = nowMs + GHOST_FUTURE_MS + 10;
    const bandFrom = lowerBound(sampleTimes.length, (i) => sampleTimes[i], minBandT);
    const bandTo = lowerBound(sampleTimes.length, (i) => sampleTimes[i], maxBandT + 1e-9);
    drawTraceBand(
      ctx,
      state.curve,
      state.tolerance,
      sampleTimes,
      bandColor(state.inBand, theme),
      xOf,
      yOf,
      bandFrom,
      bandTo,
    );
  }

  // 5. Target line via drawTargetLine; the dash phase follows rep time
  if (hasPoints) {
    const fromMs = Math.max(-GO_LEAD_MS, nowMs - GHOST_PAST_MS);
    const toMs = Math.min(durationMs, nowMs + GHOST_FUTURE_MS);
    if (fromMs < toMs) {
      drawTargetLine(
        ctx,
        state.curve,
        fromMs,
        toMs,
        theme.text,
        xOf,
        yOf,
        xOf(fromMs) - xOf(-GO_LEAD_MS),
      );
    }
  }

  // 6. User line via drawUserLine, from the first frame at or after minUserT
  if (hasPoints && state.user.length > 0) {
    const minUserT = nowMs - GHOST_PAST_MS - 50;
    const userFrom = lowerBound(state.user.length, (i) => state.user[i][0], minUserT);
    const color = state.pedal === "brake" ? theme.brake : theme.throttle;
    drawUserLine(ctx, state.user, color, xOf, yOf, userFrom);
  }

  // 7. Now-line: 2 px solid theme.accent vertical line at ghostNowX(width)
  const nowX = ghostNowX(width);
  ctx.strokeStyle = theme.accent;
  ctx.lineWidth = 2;
  ctx.setLineDash([]);
  ctx.beginPath();
  ctx.moveTo(nowX, paddingTop);
  ctx.lineTo(nowX, height - paddingBottom);
  ctx.stroke();

  // 8. Target % label at the now-line
  if (hasPoints) {
    const v = state.curve.valueAt(nowMs);
    const targetY = yOf(v);

    ctx.fillStyle = theme.accent;
    ctx.beginPath();
    ctx.arc(nowX, targetY, 4, 0, Math.PI * 2);
    ctx.fill();

    const clampedY = Math.max(paddingTop + 8, Math.min(height - paddingBottom - 8, targetY));
    ctx.font = 'bold 13px "Inter", system-ui, sans-serif';
    ctx.fillStyle = theme.text;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.fillText(`${formatPercentValue(v, state.decimals ?? 0)}%`, nowX + 8, clampedY);
  }
}
