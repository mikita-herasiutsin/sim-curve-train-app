import { bandColor } from "./barsDraw";
import { getGraphY } from "./geometry";
import { computeHorizontalGridLines } from "./graphDraw";
import type { AppThemeColors } from "./theme";

/** Target fraction (0..1) at `tMs` ms into the rep. Points are [ms, percent 0..100].
 *  Linear between points; before the first point returns the first value, after the last the last value;
 *  empty -> 0; non-finite tMs -> first value. Same rules as Rust TraceCurve::value_at. */
export function traceTargetAt(points: [number, number][], tMs: number): number {
  if (points.length === 0) {
    return 0;
  }
  if (points.length === 1) {
    return points[0][1] / 100;
  }

  const firstT = points[0][0];
  if (!Number.isFinite(tMs) || tMs <= firstT) {
    return points[0][1] / 100;
  }

  const lastT = points[points.length - 1][0];
  if (tMs >= lastT) {
    return points[points.length - 1][1] / 100;
  }

  let low = 0;
  let high = points.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (points[mid][0] <= tMs) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  const idx = low;
  const [t0, v0] = points[idx - 1];
  const [t1, v1] = points[idx];
  const dt = t1 - t0;
  if (dt <= 0) {
    return v0 / 100;
  }
  const factor = (tMs - t0) / dt;
  const val = v0 + factor * (v1 - v0);
  return val / 100;
}

/** Rep duration in ms: time of the last point, 0 when empty. */
export function traceDurationMs(points: [number, number][]): number {
  if (points.length === 0) return 0;
  return points[points.length - 1][0];
}

/** X pixel for rep time tMs on a plot spanning [0, durationMs] across [padLeft, width - padRight]. Not clamped. */
export function traceX(
  tMs: number,
  durationMs: number,
  width: number,
  padLeft: number,
  padRight: number,
): number {
  const usableW = width - padLeft - padRight;
  if (durationMs <= 0 || usableW <= 0) {
    return padLeft;
  }
  return padLeft + (tMs / durationMs) * usableW;
}

export interface TraceViewState {
  points: [number, number][];
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

  const durationMs = traceDurationMs(state.points);

  // Vertical grid lines every 500 ms labelled in seconds ("0.5 s")
  if (durationMs > 0 && width > 0) {
    ctx.strokeStyle = theme.border;
    ctx.setLineDash([4, 4]);
    ctx.textAlign = "center";
    ctx.textBaseline = "bottom";
    ctx.fillStyle = theme.textMuted;

    for (let tMs = 500; tMs <= durationMs; tMs += 500) {
      const x = traceX(tMs, durationMs, width, 0, 0);
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
  // Sample the curve at every point time plus every 10 ms in between so that clamping at 0 and 1 is drawn correctly
  if (state.points.length >= 2 && durationMs > 0) {
    const timeSet = new Set<number>();
    for (const p of state.points) {
      timeSet.add(p[0]);
    }
    for (let t = 0; t <= durationMs; t += 10) {
      timeSet.add(t);
    }
    timeSet.add(durationMs);
    const sampleTimes = Array.from(timeSet).sort((a, b) => a - b);

    ctx.fillStyle = bandColor(state.inBand, theme);
    ctx.beginPath();
    for (let i = 0; i < sampleTimes.length; i++) {
      const t = sampleTimes[i];
      const target = traceTargetAt(state.points, t);
      const topVal = Math.min(1, target + state.tolerance);
      const x = traceX(t, durationMs, width, 0, 0);
      const y = getGraphY(topVal, height, paddingTop, paddingBottom);
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    }
    for (let i = sampleTimes.length - 1; i >= 0; i--) {
      const t = sampleTimes[i];
      const target = traceTargetAt(state.points, t);
      const botVal = Math.max(0, target - state.tolerance);
      const x = traceX(t, durationMs, width, 0, 0);
      const y = getGraphY(botVal, height, paddingTop, paddingBottom);
      ctx.lineTo(x, y);
    }
    ctx.closePath();
    ctx.fill();
  }

  // Target curve as a 2 px dashed line in theme.text
  if (state.points.length > 0 && durationMs > 0) {
    ctx.strokeStyle = theme.text;
    ctx.lineWidth = 2;
    ctx.setLineDash([4, 4]);
    ctx.beginPath();
    for (let i = 0; i < state.points.length; i++) {
      const [pT, pPct] = state.points[i];
      const x = traceX(pT, durationMs, width, 0, 0);
      const y = getGraphY(pPct / 100, height, paddingTop, paddingBottom);
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
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
      const x = traceX(userT, durationMs, width, 0, 0);
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
    const x = traceX(state.playheadMs, durationMs, width, 0, 0);
    ctx.strokeStyle = theme.accent;
    ctx.lineWidth = 2;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.moveTo(x, paddingTop);
    ctx.lineTo(x, height - paddingBottom);
    ctx.stroke();
  }
}
