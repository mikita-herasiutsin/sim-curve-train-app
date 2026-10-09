import { describe, expect, it, vi } from "vitest";
import {
  drawTrace,
  GO_LEAD_MS,
  TraceCurve,
  traceDurationMs,
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

describe("TraceCurve", () => {
  it("handles empty points and single point", () => {
    const emptyCurve = new TraceCurve([]);
    expect(emptyCurve.durationMs).toBe(0);
    expect(emptyCurve.valueAt(500)).toBe(0);
    expect(emptyCurve.envelopeAt(500)).toEqual([0, 0]);

    const singleCurve = new TraceCurve([[100, 60]]);
    expect(singleCurve.durationMs).toBe(100);
    expect(singleCurve.valueAt(50)).toBe(0.6);
    expect(singleCurve.valueAt(200)).toBe(0.6);
    expect(singleCurve.envelopeAt(50)).toEqual([0.6, 0.6]);
  });

  it("returns first value for non-finite or timestamps before first point", () => {
    const curve = new TraceCurve([
      [100, 40],
      [500, 80],
    ]);
    expect(curve.valueAt(Number.NaN)).toBe(0.4);
    expect(curve.valueAt(Number.POSITIVE_INFINITY)).toBe(0.4);
    expect(curve.valueAt(Number.NEGATIVE_INFINITY)).toBe(0.4);
    expect(curve.valueAt(0)).toBe(0.4);
    expect(curve.valueAt(100)).toBe(0.4);
  });

  it("returns last value for timestamps after last point", () => {
    const curve = new TraceCurve([
      [0, 0],
      [1000, 75],
    ]);
    expect(curve.valueAt(1000)).toBe(0.75);
    expect(curve.valueAt(1500)).toBe(0.75);
  });

  it("matches hairpin reference values and monotonicity", () => {
    const points: [number, number][] = [
      [0, 0],
      [150, 100],
      [300, 95],
      [600, 70],
      [1000, 40],
      [1500, 0],
    ];
    const curve = new TraceCurve(points);

    // valueAt reference values (within 1e-5)
    expect(curve.valueAt(0)).toBeCloseTo(0, 5);
    expect(curve.valueAt(75)).toBeCloseTo(0.5, 5);
    expect(curve.valueAt(150)).toBeCloseTo(1, 5);
    expect(curve.valueAt(225)).toBeCloseTo(0.985938, 5);
    expect(curve.valueAt(450)).toBeCloseTo(0.832812, 5);
    expect(curve.valueAt(800)).toBeCloseTo(0.549167, 5);
    expect(curve.valueAt(1250)).toBeCloseTo(0.151562, 5);
    expect(curve.valueAt(1500)).toBeCloseTo(0, 5);

    // Monotone: never outside [0, 1] on 0..1500 at 1 ms steps
    for (let t = 0; t <= 1500; t++) {
      const v = curve.valueAt(t);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
    }

    // envelopeAt reference values
    const [lo225, hi225] = curve.envelopeAt(225);
    expect(lo225).toBeCloseTo(0.5, 5);
    expect(hi225).toBeCloseTo(1, 5);

    const [lo450, hi450] = curve.envelopeAt(450);
    expect(lo450).toBeCloseTo(0.7, 5);
    expect(hi450).toBeCloseTo(0.95, 5);

    const [lo800, hi800] = curve.envelopeAt(800);
    expect(lo800).toBeCloseTo(0.438229, 5);
    expect(hi800).toBeCloseTo(0.661042, 5);

    const [lo1250, hi1250] = curve.envelopeAt(1250);
    expect(lo1250).toBeCloseTo(0.0292, 4);
    expect(hi1250).toBeCloseTo(0.3088, 4);

    const [lo1500, hi1500] = curve.envelopeAt(1500);
    expect(lo1500).toBeCloseTo(0, 5);
    expect(hi1500).toBeCloseTo(0.061988, 5);

    // Before the rep the envelope only reaches values within the window
    expect(curve.envelopeAt(-500)).toEqual([0, 0]);
    expect(curve.envelopeAt(-1000)).toEqual([0, 0]);
    const [loPre, hiPre] = curve.envelopeAt(-100);
    expect(loPre).toBe(0);
    expect(hiPre).toBeCloseTo(curve.valueAt(50), 9);
    expect(curve.envelopeAt(2000)).toEqual([0, 0]);
  });

  it("matches second reference curve values", () => {
    const points: [number, number][] = [
      [0, 0],
      [880, 79],
      [1930, 63.3],
      [2790, 5],
      [3700, 0],
    ];
    const curve = new TraceCurve(points);

    // valueAt reference values
    expect(curve.valueAt(440)).toBeCloseTo(0.395, 5);
    expect(curve.valueAt(880)).toBeCloseTo(0.79, 5);
    expect(curve.valueAt(1400)).toBeCloseTo(0.7664, 4);
    expect(curve.valueAt(1930)).toBeCloseTo(0.633, 5);
    expect(curve.valueAt(2360)).toBeCloseTo(0.314745, 5);
    expect(curve.valueAt(3245)).toBeCloseTo(0.00625, 5);

    // envelopeAt reference values
    const [lo440, hi440] = curve.envelopeAt(440);
    expect(lo440).toBeCloseTo(0.200836, 5);
    expect(hi440).toBeCloseTo(0.589164, 5);

    const [lo880, hi880] = curve.envelopeAt(880);
    expect(lo880).toBeCloseTo(0.728965, 5);
    expect(hi880).toBeCloseTo(0.79, 5);

    const [lo1400, hi1400] = curve.envelopeAt(1400);
    expect(lo1400).toBeCloseTo(0.743817, 5);
    expect(hi1400).toBeCloseTo(0.780187, 5);

    const [lo1930, hi1930] = curve.envelopeAt(1930);
    expect(lo1930).toBeCloseTo(0.547242, 5);
    expect(hi1930).toBeCloseTo(0.68729, 5);

    const [lo3245, hi3245] = curve.envelopeAt(3245);
    expect(lo3245).toBeCloseTo(0.001883, 5);
    expect(hi3245).toBeCloseTo(0.014693, 5);
  });

  it("envelope on a flat curve equals the value", () => {
    const flatCurve = new TraceCurve([
      [0, 50],
      [1000, 50],
    ]);
    expect(flatCurve.envelopeAt(500)).toEqual([0.5, 0.5]);
    expect(flatCurve.envelopeAt(0)).toEqual([0.5, 0.5]);
    expect(flatCurve.envelopeAt(1000)).toEqual([0.5, 0.5]);
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

  it("supports startMs for approach second domain mapping", () => {
    const durationMs = 1500;
    const width = 500;
    // Domain [-1000, 1500] has total duration 2500 ms
    expect(traceX(-GO_LEAD_MS, durationMs, width, 0, 0, -GO_LEAD_MS)).toBe(0);
    expect(traceX(0, durationMs, width, 0, 0, -GO_LEAD_MS)).toBe(200);
    expect(traceX(durationMs, durationMs, width, 0, 0, -GO_LEAD_MS)).toBe(500);
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
    curve: new TraceCurve([
      [0, 0],
      [500, 100],
      [1000, 50],
      [1500, 0],
    ]),
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
