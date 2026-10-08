import { describe, expect, it } from "vitest";
import { applyDrillEvent, IDLE_VIEW, type SetSummary } from "./drill";

const summary: SetSummary = {
  repTotals: [80],
  best: 80,
  average: 80,
  grade: "B",
  consistency: null,
  stdDev: 0,
};

describe("applyDrillEvent", () => {
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
    const score = { kind: "trace", total: 90, grade: "A", rmse: 0 } as const;
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
