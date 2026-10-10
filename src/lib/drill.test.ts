import { describe, expect, it } from "vitest";
import {
  applyDrillEvent,
  DEFAULT_TOLERANCE,
  IDLE_VIEW,
  isPlayable,
  playableDrills,
  toleranceOf,
  type HoldDrill,
  type Overlap,
  type Preset,
  type SetSummary,
  type TraceDrill,
  type TraceScore,
} from "./drill";

function traceScore(total: number, grade: string): TraceScore {
  return {
    kind: "trace",
    total,
    grade,
    accuracy: 0,
    timing: 0,
    smoothness: 0,
    lagMs: 0,
    timeInBand: 0,
    rmse: 0,
    overshoot: 0,
    ldljUser: 0,
    ldljTarget: 0,
  };
}

const summary: SetSummary = {
  repTotals: [80],
  best: 80,
  average: 80,
  grade: "B",
  consistency: null,
  stdDev: 0,
};

describe("applyDrillEvent", () => {
  it("initializes repStartUs to 0, throttleHoldEndsUs to 0, and lastOverlap to null", () => {
    expect(IDLE_VIEW.repStartUs).toBe(0);
    expect(IDLE_VIEW.throttleHoldEndsUs).toBe(0);
    expect(IDLE_VIEW.lastOverlap).toBeNull();
  });

  it("sets repStartUs when a rep starts", () => {
    const view = applyDrillEvent(IDLE_VIEW, { event: "repStarted", rep: 0, startUs: 1000 });
    expect(view.repStartUs).toBe(1000);
  });

  it("tracks the countdown", () => {
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "countdownStarted",
      rep: 2,
      startUs: 10,
      endsUs: 3010,
    });
    expect(view).toMatchObject({ runState: "countdown", currentRep: 2, countdownEndsUs: 3010 });
  });

  it("handles throttleWait", () => {
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "throttleWait",
      rep: 1,
      sinceUs: 2000,
    });
    expect(view).toMatchObject({
      runState: "throttle",
      currentRep: 1,
      throttleHoldEndsUs: 0,
    });
  });

  it("handles throttleHoldStarted", () => {
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "throttleHoldStarted",
      rep: 1,
      startUs: 2000,
      endsUs: 3500,
    });
    expect(view).toMatchObject({
      runState: "throttle",
      currentRep: 1,
      throttleHoldEndsUs: 3500,
    });
  });

  it("resets throttleHoldEndsUs to 0 on countdownStarted", () => {
    const holdingView = applyDrillEvent(IDLE_VIEW, {
      event: "throttleHoldStarted",
      rep: 0,
      startUs: 1000,
      endsUs: 3500,
    });
    expect(holdingView.throttleHoldEndsUs).toBe(3500);

    const view = applyDrillEvent(holdingView, {
      event: "countdownStarted",
      rep: 1,
      startUs: 4000,
      endsUs: 6000,
    });
    expect(view.throttleHoldEndsUs).toBe(0);
  });

  it("resets throttleHoldEndsUs to 0 on repStarted", () => {
    const holdingView = applyDrillEvent(IDLE_VIEW, {
      event: "throttleHoldStarted",
      rep: 0,
      startUs: 1000,
      endsUs: 3500,
    });
    expect(holdingView.throttleHoldEndsUs).toBe(3500);

    const view = applyDrillEvent(holdingView, {
      event: "repStarted",
      rep: 0,
      startUs: 3500,
    });
    expect(view.throttleHoldEndsUs).toBe(0);
  });

  it("clears lastOverlap when repStarted fires", () => {
    const overlap: Overlap = { overlapMs: 45, peakThrottle: 0.3 };
    let view = applyDrillEvent(IDLE_VIEW, {
      event: "repScored",
      rep: 0,
      score: traceScore(90, "A"),
      overlap,
    });
    expect(view.lastOverlap).toEqual(overlap);

    view = applyDrillEvent(view, { event: "repStarted", rep: 1, startUs: 4000 });
    expect(view.lastOverlap).toBeNull();
  });

  it("records overlap on repScored", () => {
    const overlap: Overlap = { overlapMs: 50, peakThrottle: 0.4 };
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "repScored",
      rep: 0,
      score: traceScore(95, "S"),
      overlap,
    });
    expect(view.lastOverlap).toEqual(overlap);
  });

  it("sets lastOverlap to null on repScored when omitted", () => {
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "repScored",
      rep: 0,
      score: traceScore(95, "S"),
    });
    expect(view.lastOverlap).toBeNull();
  });

  it("records overlap on repFailed", () => {
    const overlap: Overlap = { overlapMs: 120, peakThrottle: 0.8 };
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "repFailed",
      rep: 0,
      overlap,
    });
    expect(view.lastOverlap).toEqual(overlap);
  });

  it("sets lastOverlap to null on repFailed when omitted", () => {
    const view = applyDrillEvent(IDLE_VIEW, {
      event: "repFailed",
      rep: 0,
    });
    expect(view.lastOverlap).toBeNull();
  });

  it("keeps lastOverlap null throughout a no-lead-in event sequence", () => {
    let view = applyDrillEvent(IDLE_VIEW, {
      event: "countdownStarted",
      rep: 0,
      startUs: 0,
      endsUs: 2000,
    });
    expect(view.lastOverlap).toBeNull();

    view = applyDrillEvent(view, { event: "repStarted", rep: 0, startUs: 2000 });
    expect(view.lastOverlap).toBeNull();

    view = applyDrillEvent(view, {
      event: "repScored",
      rep: 0,
      score: traceScore(88, "A"),
    });
    expect(view.lastOverlap).toBeNull();

    view = applyDrillEvent(view, {
      event: "countdownStarted",
      rep: 1,
      startUs: 3500,
      endsUs: 5500,
    });
    expect(view.lastOverlap).toBeNull();

    view = applyDrillEvent(view, { event: "repStarted", rep: 1, startUs: 5500 });
    expect(view.lastOverlap).toBeNull();

    view = applyDrillEvent(view, { event: "repFailed", rep: 1 });
    expect(view.lastOverlap).toBeNull();

    view = applyDrillEvent(view, { event: "setFinished", summary });
    expect(view.lastOverlap).toBeNull();
  });

  it("keeps the rep score through the rest countdown and clears it when the next rep starts", () => {
    const score = traceScore(90, "A");
    let view = applyDrillEvent(IDLE_VIEW, { event: "repStarted", rep: 0, startUs: 0 });
    view = applyDrillEvent(view, { event: "repScored", rep: 0, score });
    view = applyDrillEvent(view, {
      event: "countdownStarted",
      rep: 1,
      startUs: 5,
      endsUs: 3005,
    });
    expect(view).toMatchObject({ runState: "countdown", currentRep: 1, lastScore: score });
    view = applyDrillEvent(view, { event: "repStarted", rep: 1, startUs: 3005 });
    expect(view.lastScore).toBeNull();
  });

  it("moves from an aborted set to finished with its summary", () => {
    let view = applyDrillEvent(IDLE_VIEW, { event: "repStarted", rep: 1, startUs: 0 });
    view = applyDrillEvent(view, { event: "setFinished", summary });
    expect(view).toMatchObject({ runState: "finished", summary });
  });

  it("marks a failed rep as scored without a score", () => {
    const view = applyDrillEvent(IDLE_VIEW, { event: "repFailed", rep: 1 });
    expect(view).toMatchObject({ runState: "scored", currentRep: 1, lastScore: null });
  });

  it("lists every rep by its own number, failed ones included", () => {
    let view = applyDrillEvent(IDLE_VIEW, {
      event: "repScored",
      rep: 0,
      score: traceScore(90, "A"),
    });
    view = applyDrillEvent(view, { event: "repFailed", rep: 1 });
    view = applyDrillEvent(view, {
      event: "repScored",
      rep: 2,
      score: traceScore(70, "C"),
    });
    expect(view.reps).toEqual([
      { rep: 0, total: 90 },
      { rep: 1, total: null },
      { rep: 2, total: 70 },
    ]);
  });

  it("finishes with a summary", () => {
    const view = applyDrillEvent(IDLE_VIEW, { event: "setFinished", summary });
    expect(view).toMatchObject({ runState: "finished", summary });
  });

  it("finishes with a null summary when no rep was scored", () => {
    const view = applyDrillEvent(IDLE_VIEW, { event: "setFinished", summary: null });
    expect(view.runState).toBe("finished");
    expect(view.summary).toBeNull();
  });
});

describe("toleranceOf", () => {
  const base: HoldDrill = {
    id: "d",
    name: "D",
    pedal: "brake",
    reps: 5,
    leadInMs: 3000,
    type: "hold",
    target: 70,
    holdMs: 2000,
  };

  it("uses the drill's tolerance when set", () => {
    expect(toleranceOf({ ...base, tolerance: 5 })).toBe(5);
  });

  it("falls back to the D-17 default when omitted", () => {
    expect(DEFAULT_TOLERANCE).toBe(10);
    expect(toleranceOf(base)).toBe(10);
  });
});

describe("isPlayable and playableDrills", () => {
  const holdBrake: HoldDrill = {
    id: "hb",
    name: "Hold Brake",
    pedal: "brake",
    reps: 3,
    leadInMs: 1000,
    type: "hold",
    target: 50,
    holdMs: 1000,
  };
  const traceThrottle: TraceDrill = {
    id: "tt",
    name: "Trace Throttle",
    pedal: "throttle",
    reps: 3,
    leadInMs: 1000,
    type: "trace",
    points: [
      [0, 0],
      [1000, 100],
    ],
  };
  const traceClutch: TraceDrill = {
    id: "tc",
    name: "Trace Clutch",
    pedal: "clutch",
    reps: 3,
    leadInMs: 1000,
    type: "trace",
    points: [
      [0, 0],
      [1000, 100],
    ],
  };
  const holdClutch: HoldDrill = {
    id: "hc",
    name: "Hold Clutch",
    pedal: "clutch",
    reps: 3,
    leadInMs: 1000,
    type: "hold",
    target: 50,
    holdMs: 1000,
  };

  it("considers hold and trace drills on brake and throttle playable", () => {
    expect(isPlayable(holdBrake)).toBe(true);
    expect(isPlayable(traceThrottle)).toBe(true);
  });

  it("rejects clutch drills as unplayable", () => {
    expect(isPlayable(traceClutch)).toBe(false);
    expect(isPlayable(holdClutch)).toBe(false);
  });

  it("filters unplayable drills from a preset", () => {
    const preset: Preset = {
      schemaVersion: 1,
      id: "test",
      name: "Test",
      description: "",
      drills: [holdBrake, traceThrottle, traceClutch, holdClutch],
    };
    expect(playableDrills(preset)).toEqual([holdBrake, traceThrottle]);
  });
});
