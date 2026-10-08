import { describe, expect, it } from "vitest";
import { applyDrillEvent, IDLE_VIEW, type SetSummary } from "./drill";

const summary: SetSummary = {
  repsCount: 1,
  repTotals: [80],
  bestTotal: 80,
  avgTotal: 80,
  consistency: 100,
};

describe("applyDrillEvent", () => {
  it("tracks the countdown and clears the previous score", () => {
    const view = applyDrillEvent(
      { ...IDLE_VIEW, lastScore: { kind: "trace", total: 1, grade: "A", rmse: 0 } },
      { event: "countdownStarted", rep: 2, startUs: 10, endsUs: 3010 },
    );
    expect(view).toMatchObject({ runState: "countdown", currentRep: 2, countdownEndsUs: 3010 });
    expect(view.lastScore).toBeNull();
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
