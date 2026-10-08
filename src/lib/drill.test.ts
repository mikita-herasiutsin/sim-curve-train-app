import { describe, expect, it } from "vitest";
import { applyDrillEvent, IDLE_VIEW, type SetSummary, type TraceScore } from "./drill";

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
  it("initializes repStartUs to 0", () => {
    expect(IDLE_VIEW.repStartUs).toBe(0);
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
