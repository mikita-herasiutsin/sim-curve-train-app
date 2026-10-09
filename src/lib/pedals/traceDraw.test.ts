import { describe, expect, it, vi } from "vitest";
import {
  drawTrace,
  traceDurationMs,
  traceTargetAt,
  traceX,
  type TraceViewState,
} from "./traceDraw";
import { DEFAULT_DARK_THEME_COLORS as theme } from "./theme";

describe("traceDurationMs", () => {
  it("returns 0 when points array is empty", () => {
    expect(traceDurationMs([])).toBe(0);
  });

  it("returns the timestamp of the last point", () => {
    expect(
      traceDurationMs([
        [0, 0],
        [500, 80],
        [1500, 0],
      ]),
    ).toBe(1500);
  });
});

describe("traceTargetAt", () => {
  it("returns 0 for empty points", () => {
    expect(traceTargetAt([], 500)).toBe(0);
  });

  it("returns single point value as fraction", () => {
    expect(traceTargetAt([[100, 60]], 50)).toBe(0.6);
    expect(traceTargetAt([[100, 60]], 200)).toBe(0.6);
  });

  it("returns first value for non-finite or timestamps before first point", () => {
    const points: [number, number][] = [
      [100, 40],
      [500, 80],
    ];
    expect(traceTargetAt(points, Number.NaN)).toBe(0.4);
    expect(traceTargetAt(points, Number.POSITIVE_INFINITY)).toBe(0.4);
    expect(traceTargetAt(points, Number.NEGATIVE_INFINITY)).toBe(0.4);
    expect(traceTargetAt(points, 0)).toBe(0.4);
    expect(traceTargetAt(points, 100)).toBe(0.4);
  });

  it("returns last value for timestamps after last point", () => {
    const points: [number, number][] = [
      [0, 0],
      [1000, 75],
    ];
    expect(traceTargetAt(points, 1000)).toBe(0.75);
    expect(traceTargetAt(points, 1500)).toBe(0.75);
  });

  it("interpolates linearly between points", () => {
    const points: [number, number][] = [
      [0, 0],
      [100, 100],
      [300, 50],
    ];
    expect(traceTargetAt(points, 50)).toBeCloseTo(0.5);
    expect(traceTargetAt(points, 100)).toBeCloseTo(1.0);
    expect(traceTargetAt(points, 200)).toBeCloseTo(0.75);
    expect(traceTargetAt(points, 300)).toBeCloseTo(0.5);
  });

  it("mirrors hairpin curve interpolation", () => {
    const points: [number, number][] = [
      [0, 0],
      [150, 100],
      [300, 95],
      [600, 70],
      [1000, 40],
      [1500, 0],
    ];
    expect(traceTargetAt(points, 75)).toBeCloseTo(0.5);
    expect(traceTargetAt(points, 150)).toBeCloseTo(1.0);
    expect(traceTargetAt(points, 225)).toBeCloseTo(0.975);
    expect(traceTargetAt(points, 1500)).toBeCloseTo(0.0);
  });
});

describe("traceX", () => {
  it("projects tMs onto width between padLeft and width - padRight", () => {
    expect(traceX(0, 1000, 500, 0, 0)).toBe(0);
    expect(traceX(500, 1000, 500, 0, 0)).toBe(250);
    expect(traceX(1000, 1000, 500, 0, 0)).toBe(500);
  });

  it("accounts for padding", () => {
    expect(traceX(0, 1000, 500, 50, 50)).toBe(50);
    expect(traceX(500, 1000, 500, 50, 50)).toBe(250);
    expect(traceX(1000, 1000, 500, 50, 50)).toBe(450);
  });

  it("is not clamped", () => {
    expect(traceX(-200, 1000, 500, 0, 0)).toBe(-100);
    expect(traceX(1200, 1000, 500, 0, 0)).toBe(600);
  });

  it("returns padLeft when durationMs <= 0", () => {
    expect(traceX(100, 0, 500, 20, 20)).toBe(20);
  });
});

describe("drawTrace", () => {
  function makeMockContext(): CanvasRenderingContext2D {
    return {
      clearRect: vi.fn(),
      fillRect: vi.fn(),
      fillText: vi.fn(),
      beginPath: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      stroke: vi.fn(),
      fill: vi.fn(),
      closePath: vi.fn(),
      setLineDash: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
    } as unknown as CanvasRenderingContext2D;
  }

  const baseState: TraceViewState = {
    points: [
      [0, 0],
      [500, 100],
      [1000, 50],
      [1500, 0],
    ],
    pedal: "brake",
    tolerance: 0.06,
    playheadMs: 500,
    user: [
      [0, 0],
      [250, 0.4],
      [500, 0.95],
    ],
    inBand: true,
  };

  it("renders background, grids, tolerance band, target curve, user trace, and playhead", () => {
    const ctx = makeMockContext();
    drawTrace(ctx, 600, 400, baseState, theme);

    expect(ctx.clearRect).toHaveBeenCalledWith(0, 0, 600, 400);
    expect(ctx.fillRect).toHaveBeenCalledWith(0, 0, 600, 400);

    // Horizontal grid labels: 0%, 25%, 50%, 75%, 100%
    expect(ctx.fillText).toHaveBeenCalledWith("0%", expect.any(Number), expect.any(Number));
    expect(ctx.fillText).toHaveBeenCalledWith("100%", expect.any(Number), expect.any(Number));

    // Vertical grid labels: 500ms -> "0.5 s", 1000ms -> "1 s"
    expect(ctx.fillText).toHaveBeenCalledWith("0.5 s", expect.any(Number), expect.any(Number));
    expect(ctx.fillText).toHaveBeenCalledWith("1 s", expect.any(Number), expect.any(Number));

    // Tolerance band was closed and filled
    expect(ctx.closePath).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();

    // Playhead was stroked
    expect(ctx.stroke).toHaveBeenCalled();
  });

  it("hides the playhead when playheadMs is null", () => {
    const ctx = makeMockContext();
    drawTrace(ctx, 600, 400, { ...baseState, playheadMs: null }, theme);
    // ctx.strokeStyle should not end on theme.accent
    expect(ctx.strokeStyle).not.toBe(theme.accent);
  });

  it("colors user trace according to pedal", () => {
    const ctxBrake = makeMockContext();
    drawTrace(ctxBrake, 600, 400, { ...baseState, pedal: "brake", playheadMs: null }, theme);
    expect(ctxBrake.strokeStyle).toBe(theme.brake);

    const ctxThrottle = makeMockContext();
    drawTrace(ctxThrottle, 600, 400, { ...baseState, pedal: "throttle", playheadMs: null }, theme);
    expect(ctxThrottle.strokeStyle).toBe(theme.throttle);
  });
});
