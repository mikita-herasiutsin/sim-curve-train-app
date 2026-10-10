import { describe, expect, it } from "vitest";
import type { HoldDrill, Preset, TraceDrill } from "./drill";
import type { WarmUpStepResult } from "./attempts";
import {
  buildWarmUpRun,
  completeStep,
  currentDrill,
  hasWarmUp,
  isDone,
  skipStep,
  startWarmUp,
  warmUpPlan,
  warmUpScore,
} from "./warmup";

const holdBrake: HoldDrill = {
  id: "hb",
  name: "Hold Brake",
  pedal: "brake",
  reps: 5,
  leadInMs: 1000,
  type: "hold",
  target: 50,
  holdMs: 1000,
};

const traceThrottle: TraceDrill = {
  id: "tt",
  name: "Trace Throttle",
  pedal: "throttle",
  reps: 5,
  leadInMs: 1000,
  type: "trace",
  points: [
    [0, 0],
    [1000, 100],
  ],
};

const holdThrottle: HoldDrill = {
  id: "ht",
  name: "Hold Throttle",
  pedal: "throttle",
  reps: 5,
  leadInMs: 1000,
  type: "hold",
  target: 70,
  holdMs: 1000,
};

const traceClutch: TraceDrill = {
  id: "tc",
  name: "Trace Clutch",
  pedal: "clutch",
  reps: 5,
  leadInMs: 1000,
  type: "trace",
  points: [
    [0, 0],
    [1000, 100],
  ],
};

describe("hasWarmUp and warmUpPlan", () => {
  it("detects whether a preset has a usable warm-up", () => {
    const noWarmUp: Preset = {
      schemaVersion: 1,
      id: "p1",
      name: "P1",
      description: "",
      drills: [holdBrake, traceThrottle],
    };
    expect(hasWarmUp(noWarmUp)).toBe(false);
    expect(warmUpPlan(noWarmUp, "2026-10-10T12:00:00Z")).toBeNull();

    const emptySteps: Preset = {
      ...noWarmUp,
      warmUp: { steps: [] },
    };
    expect(hasWarmUp(emptySteps)).toBe(false);
    expect(warmUpPlan(emptySteps, "2026-10-10T12:00:00Z")).toBeNull();

    const unplayableOnly: Preset = {
      schemaVersion: 1,
      id: "p2",
      name: "P2",
      description: "",
      drills: [traceClutch],
      warmUp: {
        steps: [
          { drill: "tc", reps: 3 },
          { drill: "missing-drill", reps: 2 },
        ],
      },
    };
    expect(hasWarmUp(unplayableOnly)).toBe(false);
    expect(warmUpPlan(unplayableOnly, "2026-10-10T12:00:00Z")).toBeNull();

    const validPreset: Preset = {
      schemaVersion: 1,
      id: "p3",
      name: "P3",
      description: "",
      drills: [holdBrake, traceThrottle, traceClutch],
      warmUp: {
        steps: [
          { drill: "hb", reps: 2 },
          { drill: "tc", reps: 4 },
          { drill: "missing", reps: 1 },
          { drill: "tt", reps: 3 },
        ],
      },
    };
    expect(hasWarmUp(validPreset)).toBe(true);

    const plan = warmUpPlan(validPreset, "2026-10-10T12:00:00Z");
    expect(plan).not.toBeNull();
    expect(plan?.presetId).toBe("p3");
    expect(plan?.startedAt).toBe("2026-10-10T12:00:00Z");
    // Order preserved, clutch and missing dropped, reps overridden
    expect(plan?.drills).toEqual([
      { ...holdBrake, reps: 2 },
      { ...traceThrottle, reps: 3 },
    ]);
    // Original drill reps unaffected
    expect(holdBrake.reps).toBe(5);
    expect(traceThrottle.reps).toBe(5);
  });
});

describe("warm-up execution flow", () => {
  const preset: Preset = {
    schemaVersion: 1,
    id: "p-walk",
    name: "Walk",
    description: "",
    drills: [holdBrake, traceThrottle, holdThrottle],
    warmUp: {
      steps: [
        { drill: "hb", reps: 2 },
        { drill: "tt", reps: 3 },
        { drill: "ht", reps: 1 },
      ],
    },
  };

  it("walks a 3-step plan with complete, skip, complete and verifies immutability", () => {
    const plan = warmUpPlan(preset, "2026-10-10T14:00:00Z");
    expect(plan).not.toBeNull();
    if (!plan) return;

    const initial = startWarmUp(plan);
    expect(initial.index).toBe(0);
    expect(initial.results).toEqual([]);
    expect(isDone(initial)).toBe(false);
    expect(currentDrill(initial)).toEqual({ ...holdBrake, reps: 2 });

    // Step 1: Complete
    const step1 = completeStep(initial, { attemptId: 10, score: 85.5 });
    expect(step1.index).toBe(1);
    expect(step1.results).toEqual([{ drillId: "hb", skipped: false, attemptId: 10, score: 85.5 }]);
    expect(isDone(step1)).toBe(false);
    expect(currentDrill(step1)).toEqual({ ...traceThrottle, reps: 3 });

    // Step 2: Skip
    const step2 = skipStep(step1, 11);
    expect(step2.index).toBe(2);
    expect(step2.results).toEqual([
      { drillId: "hb", skipped: false, attemptId: 10, score: 85.5 },
      { drillId: "tt", skipped: true, attemptId: 11, score: null },
    ]);
    expect(isDone(step2)).toBe(false);
    expect(currentDrill(step2)).toEqual({ ...holdThrottle, reps: 1 });

    // Step 3: Complete
    const step3 = completeStep(step2, { attemptId: 12, score: 92 });
    expect(step3.index).toBe(3);
    expect(step3.results).toEqual([
      { drillId: "hb", skipped: false, attemptId: 10, score: 85.5 },
      { drillId: "tt", skipped: true, attemptId: 11, score: null },
      { drillId: "ht", skipped: false, attemptId: 12, score: 92 },
    ]);
    expect(isDone(step3)).toBe(true);
    expect(currentDrill(step3)).toBeNull();

    // After done, completeStep and skipStep change nothing
    expect(completeStep(step3, { attemptId: 99, score: 100 })).toBe(step3);
    expect(skipStep(step3, 99)).toBe(step3);

    // Verify inputs were not mutated
    expect(initial.index).toBe(0);
    expect(initial.results).toEqual([]);
    expect(step1.index).toBe(1);
    expect(step1.results.length).toBe(1);
    expect(step2.index).toBe(2);
    expect(step2.results.length).toBe(2);

    // Build warm-up run
    const run = buildWarmUpRun(step3);
    expect(run).toEqual({
      presetId: "p-walk",
      startedAt: "2026-10-10T14:00:00Z",
      steps: step3.results,
    });
  });
});

describe("warmUpScore", () => {
  it("returns 0 for empty results", () => {
    expect(warmUpScore([])).toBe(0);
  });

  it("calculates mean with skipped and null scoring 0", () => {
    const results: WarmUpStepResult[] = [
      { drillId: "d1", skipped: false, attemptId: 1, score: 80 },
      { drillId: "d2", skipped: true, attemptId: 2, score: 90 }, // skipped: score ignored
      { drillId: "d3", skipped: false, attemptId: 3, score: 60 },
      { drillId: "d4", skipped: false, attemptId: null, score: null },
    ];
    // (80 + 0 + 60 + 0) / 4 = 35
    expect(warmUpScore(results)).toBe(35);
  });
});
