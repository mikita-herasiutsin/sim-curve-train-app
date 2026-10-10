import type { Drill, Preset } from "./drill";
import { isPlayable } from "./drill";
import type { NewWarmUpRun, WarmUpStepResult } from "./attempts";

export interface WarmUpPlan {
  presetId: string;
  startedAt: string;
  /** In step order, each with `reps` set to the step's reps. */
  drills: Drill[];
}

export interface WarmUpProgress {
  plan: WarmUpPlan;
  /** Index of the current step; equals drills.length when done. */
  index: number;
  results: WarmUpStepResult[];
}

/** True when warmUp has at least one step whose drill exists and isPlayable. */
export function hasWarmUp(preset: Preset): boolean {
  return warmUpPlan(preset, "") !== null;
}

/** Null without a usable warm-up; steps naming a missing or unplayable drill are dropped. */
export function warmUpPlan(preset: Preset, startedAt: string): WarmUpPlan | null {
  if (!preset.warmUp?.steps) {
    return null;
  }
  const drills: Drill[] = [];
  for (const step of preset.warmUp.steps) {
    const drill = preset.drills.find((d) => d.id === step.drill);
    if (drill && isPlayable(drill)) {
      drills.push({ ...drill, reps: step.reps });
    }
  }
  if (drills.length === 0) {
    return null;
  }
  return {
    presetId: preset.id,
    startedAt,
    drills,
  };
}

/** Starts a warm-up routine at step index 0 with empty results. */
export function startWarmUp(plan: WarmUpPlan): WarmUpProgress {
  return {
    plan,
    index: 0,
    results: [],
  };
}

/** Returns the current drill to perform, or null when the warm-up is done. */
export function currentDrill(p: WarmUpProgress): Drill | null {
  if (isDone(p)) {
    return null;
  }
  return p.plan.drills[p.index] ?? null;
}

/** Whether all steps in the warm-up plan have been completed or skipped. */
export function isDone(p: WarmUpProgress): boolean {
  return p.index >= p.plan.drills.length;
}

/** Appends a completed step result and advances; returns p unchanged when done. */
export function completeStep(
  p: WarmUpProgress,
  r: { attemptId: number | null; score: number | null },
): WarmUpProgress {
  if (isDone(p)) {
    return p;
  }
  const drill = p.plan.drills[p.index];
  return {
    plan: p.plan,
    index: p.index + 1,
    results: [
      ...p.results,
      {
        drillId: drill.id,
        reps: drill.reps,
        skipped: false,
        ...r,
      },
    ],
  };
}

/** Appends a skipped step result and advances; returns p unchanged when done. */
export function skipStep(p: WarmUpProgress, attemptId: number | null = null): WarmUpProgress {
  if (isDone(p)) {
    return p;
  }
  const drill = p.plan.drills[p.index];
  return {
    plan: p.plan,
    index: p.index + 1,
    results: [
      ...p.results,
      {
        drillId: drill.id,
        reps: drill.reps,
        skipped: true,
        attemptId,
        score: null,
      },
    ],
  };
}

/** D-26: mean of scores where skipped or null counts as 0; [] gives 0. Must match Rust `warm_up_score`. */
export function warmUpScore(results: WarmUpStepResult[]): number {
  if (results.length === 0) {
    return 0;
  }
  const total = results.reduce((acc, s) => {
    if (s.skipped) {
      return acc;
    }
    return acc + (s.score ?? 0);
  }, 0);
  return total / results.length;
}

/** Constructs a NewWarmUpRun record to persist from a completed progress object. */
export function buildWarmUpRun(p: WarmUpProgress): NewWarmUpRun {
  return {
    presetId: p.plan.presetId,
    startedAt: p.plan.startedAt,
    steps: [...p.results],
  };
}
