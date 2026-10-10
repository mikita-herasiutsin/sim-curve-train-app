import { describe, expect, it, vi } from "vitest";
import {
  drawGhostTrace,
  drawTargetLine,
  drawTrace,
  drawTraceBand,
  drawUserLine,
  ghostNowMs,
  ghostNowX,
  ghostX,
  GHOST_NOW_FRAC,
  GO_LEAD_MS,
  TraceCurve,
  traceDurationMs,
  traceViewPhase,
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

  it("matches brute-force envelope calculation for hairpin curve at every index", () => {
    const points: [number, number][] = [
      [0, 0],
      [150, 100],
      [300, 95],
      [600, 70],
      [1000, 40],
      [1500, 0],
    ];
    const curve = new TraceCurve(points);
    const window = 150;
    const len = Math.ceil(curve.durationMs) + 2 * window + 1;
    const grid = new Float64Array(len);
    for (let i = 0; i < len; i++) {
      grid[i] = curve.valueAt(i - window);
    }

    for (let i = 0; i < len; i++) {
      let expectedLo = Infinity;
      let expectedHi = -Infinity;
      for (let j = Math.max(0, i - window); j <= Math.min(len - 1, i + window); j++) {
        expectedLo = Math.min(expectedLo, grid[j]);
        expectedHi = Math.max(expectedHi, grid[j]);
      }
      const [actualLo, actualHi] = curve.envelopeAt(i - window);
      expect(actualLo).toBe(expectedLo);
      expect(actualHi).toBe(expectedHi);
    }
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
    arc: vi.fn(),
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

describe("drawTrace", () => {
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

describe("traceViewPhase", () => {
  it("returns zero shownStartUs and null playheadMs during early countdown", () => {
    const phase = traceViewPhase({
      nowUs: 1_000_000,
      countingDown: true,
      countdownEndsUs: 3_000_000, // 2s remaining > GO_LEAD_MS
      active: false,
      repStartUs: 0,
      lastShownStartUs: 500_000,
      durationMs: 1500,
    });
    expect(phase).toEqual({ shownStartUs: 0, playheadMs: null });
  });

  it("shows countdownEndsUs and negative playhead during the GO second", () => {
    const phase = traceViewPhase({
      nowUs: 2_600_000,
      countingDown: true,
      countdownEndsUs: 3_000_000, // 400ms remaining <= GO_LEAD_MS
      active: false,
      repStartUs: 0,
      lastShownStartUs: 0,
      durationMs: 1500,
    });
    expect(phase).toEqual({
      shownStartUs: 3_000_000,
      playheadMs: -400,
    });
  });

  it("tracks active mid-rep with playhead in ms", () => {
    const phase = traceViewPhase({
      nowUs: 3_500_000,
      countingDown: false,
      countdownEndsUs: 3_000_000,
      active: true,
      repStartUs: 3_000_000, // 500ms into rep
      lastShownStartUs: 3_000_000,
      durationMs: 1500,
    });
    expect(phase).toEqual({
      shownStartUs: 3_000_000,
      playheadMs: 500,
    });
  });

  it("clamps playhead to durationMs when active past the end", () => {
    const phase = traceViewPhase({
      nowUs: 5_000_000,
      countingDown: false,
      countdownEndsUs: 3_000_000,
      active: true,
      repStartUs: 3_000_000, // 2000ms > durationMs (1500)
      lastShownStartUs: 3_000_000,
      durationMs: 1500,
    });
    expect(phase).toEqual({
      shownStartUs: 3_000_000,
      playheadMs: 1500,
    });
  });

  it("keeps lastShownStartUs and nulls playhead when idle after a rep", () => {
    const phase = traceViewPhase({
      nowUs: 5_500_000,
      countingDown: false,
      countdownEndsUs: 3_000_000,
      active: false,
      repStartUs: 3_000_000,
      lastShownStartUs: 3_000_000,
      durationMs: 1500,
    });
    expect(phase).toEqual({
      shownStartUs: 3_000_000,
      playheadMs: null,
    });
  });
});

describe("ghost geometry", () => {
  it("computes ghostNowX and GHOST_NOW_FRAC", () => {
    expect(GHOST_NOW_FRAC).toBe(0.25);
    expect(ghostNowX(800)).toBe(200);
  });

  it("projects tMs to ghostX", () => {
    const width = 800;
    const nowMs = 1200;
    expect(ghostX(nowMs, nowMs, width)).toBe(0.25 * width);
    expect(ghostX(nowMs - 1000, nowMs, width)).toBe(0);
    expect(ghostX(nowMs + 3000, nowMs, width)).toBe(width);
    expect(ghostX(nowMs + 1500, nowMs, width)).toBe(0.625 * width);
    expect(ghostX(nowMs, nowMs, 0)).toBe(0);
  });

  it("uses the playhead when set and -GO_LEAD_MS when it is null", () => {
    expect(ghostNowMs(420)).toBe(420);
    expect(ghostNowMs(-500)).toBe(-500);
    expect(ghostNowMs(null)).toBe(-GO_LEAD_MS);
  });
});

interface RecordedStroke {
  strokeStyle: string;
  points: [number, number][];
}

/** Mock context that records each stroke's strokeStyle and the points of its path. */
function makeRecordingContext(): { ctx: CanvasRenderingContext2D; strokes: RecordedStroke[] } {
  const ctx = makeMockContext();
  const strokes: RecordedStroke[] = [];
  let path: [number, number][] = [];
  vi.mocked(ctx.beginPath).mockImplementation(() => {
    path = [];
  });
  vi.mocked(ctx.moveTo).mockImplementation((x: number, y: number) => {
    path.push([x, y]);
  });
  vi.mocked(ctx.lineTo).mockImplementation((x: number, y: number) => {
    path.push([x, y]);
  });
  vi.mocked(ctx.stroke).mockImplementation(() => {
    strokes.push({ strokeStyle: String(ctx.strokeStyle), points: path.slice() });
  });
  return { ctx, strokes };
}

describe("drawGhostTrace", () => {
  it("renders band, target % label, and grid lines on baseState", () => {
    const ctx = makeMockContext();
    drawGhostTrace(ctx, 600, 400, baseState, theme);

    // fills the band (closePath and fill called)
    expect(ctx.closePath).toHaveBeenCalled();
    expect(ctx.fill).toHaveBeenCalled();

    // with playheadMs 500 (curve value 100% there) calls fillText("100%", 158, any number)
    expect(ctx.fillText).toHaveBeenCalledWith("100%", 158, expect.any(Number));

    // calls fillText("0.5 s", ...) when that grid line is in view
    expect(ctx.fillText).toHaveBeenCalledWith("0.5 s", expect.any(Number), expect.any(Number));
  });

  it("draws the now-line in theme.accent at x = 150 for playheadMs 700", () => {
    // 700 is not a multiple of 500, so the 0.5 s grid line (x = 120 here) cannot match
    const { ctx, strokes } = makeRecordingContext();
    drawGhostTrace(ctx, 600, 400, { ...baseState, playheadMs: 700 }, theme);
    const accentStrokes = strokes.filter((s) => s.strokeStyle === theme.accent);
    expect(accentStrokes).toEqual([
      {
        strokeStyle: theme.accent,
        points: [
          [150, 18],
          [150, 374],
        ],
      },
    ]);
  });

  it("idle before a rep (playheadMs null, no user): label shows the value at -1000 ms and the now-line is drawn", () => {
    const curve = new TraceCurve([
      [0, 40],
      [500, 100],
      [1000, 50],
      [1500, 0],
    ]);
    const { ctx, strokes } = makeRecordingContext();
    drawGhostTrace(ctx, 600, 400, { ...baseState, curve, playheadMs: null, user: [] }, theme);

    // valueAt(-1000) clamps to the first point, 40%
    expect(ctx.fillText).toHaveBeenCalledWith("40%", 158, expect.any(Number));
    expect(strokes.filter((s) => s.strokeStyle === theme.accent)).toEqual([
      {
        strokeStyle: theme.accent,
        points: [
          [150, 18],
          [150, 374],
        ],
      },
    ]);
  });

  it("after a rep (playheadMs null, user set): no accent stroke and the band is filled", () => {
    const { ctx, strokes } = makeRecordingContext();
    drawGhostTrace(ctx, 600, 400, { ...baseState, playheadMs: null }, theme);
    expect(strokes.filter((s) => s.strokeStyle === theme.accent)).toEqual([]);
    expect(ctx.fill).toHaveBeenCalled();
    // The whole-rep view draws no target label at the now-line
    expect(vi.mocked(ctx.fillText).mock.calls.filter(([, x]) => x === 158)).toEqual([]);
  });

  it("clamps the target label to paddingTop + 8 when the curve is at 100%", () => {
    // playheadMs 500 on baseState is 100%; its y would be the top of the plot without the clamp
    const ctx = makeMockContext();
    drawGhostTrace(ctx, 600, 400, baseState, theme);
    expect(ctx.fillText).toHaveBeenCalledWith("100%", 158, 18 + 8);
  });

  it("does not throw and draws no target label for empty points and durationMs 0", () => {
    const ctx = makeMockContext();
    const state = { ...baseState, curve: new TraceCurve([]), playheadMs: null, user: [] };
    expect(() => drawGhostTrace(ctx, 600, 400, state, theme)).not.toThrow();
    expect(vi.mocked(ctx.fillText).mock.calls.filter(([, x]) => x === 158)).toEqual([]);
  });

  it("formats target % label with decimals", () => {
    const ctx = makeMockContext();
    drawGhostTrace(ctx, 600, 400, { ...baseState, decimals: 1 }, theme);
    expect(ctx.fillText).toHaveBeenCalledWith("100.0%", 158, expect.any(Number));
  });
});

describe("drawTargetLine", () => {
  it("sets lineDashOffset before stroke and resets it to 0 after", () => {
    const curve = new TraceCurve([
      [0, 0],
      [1000, 100],
    ]);
    const ctx = makeMockContext();
    const offsetsAtStroke: number[] = [];
    vi.mocked(ctx.stroke).mockImplementation(() => {
      offsetsAtStroke.push(ctx.lineDashOffset);
    });
    drawTargetLine(
      ctx,
      curve,
      0,
      1000,
      theme.text,
      (t) => t,
      (v) => v,
      7,
    );
    expect(offsetsAtStroke).toEqual([7]);
    expect(ctx.lineDashOffset).toBe(0);
  });
});

describe("drawUserLine", () => {
  it("starts its path at user[from]", () => {
    const ctx = makeMockContext();
    const user: [number, number][] = [
      [0, 0.1],
      [250, 0.4],
      [500, 0.95],
      [750, 0.6],
    ];
    drawUserLine(
      ctx,
      user,
      theme.brake,
      (t) => t,
      (v) => v,
      2,
    );
    expect(vi.mocked(ctx.moveTo).mock.calls).toEqual([[500, 0.95]]);
    expect(vi.mocked(ctx.lineTo).mock.calls).toEqual([[750, 0.6]]);
  });
});

describe("drawTraceBand", () => {
  it("on a flat curve: every moveTo/lineTo y equals yOf(value + tolerance) or yOf(value - tolerance)", () => {
    const flatCurve = new TraceCurve([
      [0, 50],
      [1000, 50],
    ]);
    const ctx = makeMockContext();
    const tolerance = 0.08;
    const yOf = (v: number) => 400 - v * 300;
    const xOf = (t: number) => t;

    drawTraceBand(
      ctx,
      flatCurve,
      tolerance,
      flatCurve.sampleTimes,
      "rgba(255, 255, 255, 0.2)",
      xOf,
      yOf,
    );

    const expectedUpperY = yOf(0.5 + tolerance);
    const expectedLowerY = yOf(0.5 - tolerance);

    const moveCalls = vi.mocked(ctx.moveTo).mock.calls;
    const lineCalls = vi.mocked(ctx.lineTo).mock.calls;
    const allCalls = [...moveCalls, ...lineCalls];

    expect(allCalls.length).toBeGreaterThan(0);
    for (const [, y] of allCalls) {
      expect(y === expectedUpperY || y === expectedLowerY).toBe(true);
    }
  });

  it("with a from/to range only visits the times in [from, to)", () => {
    const flatCurve = new TraceCurve([
      [0, 50],
      [1000, 50],
    ]);
    const ctx = makeMockContext();
    const times = [0, 100, 200, 300, 400, 500];
    drawTraceBand(
      ctx,
      flatCurve,
      0.08,
      times,
      "rgba(255, 255, 255, 0.2)",
      (t) => t,
      (v) => v,
      2,
      4,
    );

    const xs = [...vi.mocked(ctx.moveTo).mock.calls, ...vi.mocked(ctx.lineTo).mock.calls].map(
      ([x]) => x,
    );
    expect([...new Set(xs)].sort((a, b) => a - b)).toEqual([200, 300]);
    expect(ctx.fill).toHaveBeenCalled();
  });
});
